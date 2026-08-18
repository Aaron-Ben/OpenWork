use std::time::Duration;

use openwork_collab::{
    OpenCodeClient, OpenCodeServer, SpikeResult, TurnEvidence, event_session_id, message_error,
    print_turn_summary, session_id,
};
use serde_json::Value;
use tokio::time::Instant;

#[tokio::main]
async fn main() -> SpikeResult<()> {
    let mut server = OpenCodeServer::start().await?;
    let result = run(server.base_url()).await;
    server.shutdown().await;
    result
}

async fn run(base_url: &str) -> SpikeResult<()> {
    let home = tempfile::tempdir()?;
    let client = OpenCodeClient::new(base_url, home.path());
    let session = client.create_session("OpenWork P0 spike 3").await?;
    println!("session.create.response={session}");
    let id = session_id(&session)?.to_string();
    let mut events = client.event_stream().await?;

    let first_prompt = "You must call the bash tool now and run `sleep 10`. Do not simulate it and do not use another tool. After the command completes, reply with exactly FIRST_DONE.";
    let first_response = client.prompt_async(&id, first_prompt).await?;
    first_response.print("round1.prompt_async");
    if first_response.status != reqwest::StatusCode::NO_CONTENT {
        return Err(message_error("first prompt_async was not accepted"));
    }

    let deadline = Instant::now() + Duration::from_secs(90);
    let mut turn = TurnEvidence::default();
    let running_tool_event = loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(message_error(
                "did not observe a running bash tool, so session busy state was not proven",
            ));
        }
        let event = events.next_json(remaining).await?;
        let is_running_bash = event_session_id(&event) == Some(id.as_str())
            && event.get("type").and_then(Value::as_str) == Some("message.part.updated")
            && event
                .pointer("/properties/part/type")
                .and_then(Value::as_str)
                == Some("tool")
            && event
                .pointer("/properties/part/tool")
                .and_then(Value::as_str)
                == Some("bash")
            && event
                .pointer("/properties/part/state/status")
                .and_then(Value::as_str)
                == Some("running");
        turn.observe(&id, event.clone());
        if is_running_bash {
            break event;
        }
        if turn.saw_idle {
            return Err(message_error(
                "first round became idle before a running bash tool was observed",
            ));
        }
    };
    println!("busy.proof.running_tool={running_tool_event}");

    let second_prompt =
        "This is SECOND_PROMPT. Reply with exactly SECOND_DONE when you process it.";
    let second_response = client.prompt_async(&id, second_prompt).await?;
    second_response.print("round2.while_busy.prompt_async");

    let deadline = Instant::now() + Duration::from_secs(240);
    while !turn.saw_idle {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(message_error("busy-session experiment did not reach idle"));
        }
        let event = events.next_json(remaining).await?;
        turn.observe(&id, event);
    }
    print_turn_summary("combined_turn", &turn);
    let messages = client.get_messages(&id).await?;
    println!("session.messages.after_idle={messages}");

    if !second_response.status.is_success() {
        println!("SPIKE3_OUTCOME=报错");
        println!("SPIKE3_ERROR_STATUS={}", second_response.status);
        println!("SPIKE3_ERROR_BODY={}", second_response.body);
        return Ok(());
    }

    if let Some(error) = turn
        .events
        .iter()
        .find(|event| event.get("type").and_then(Value::as_str) == Some("session.error"))
    {
        println!("SPIKE3_OUTCOME=报错");
        println!("SPIKE3_ERROR_EVENT={error}");
        return Ok(());
    }

    let second_user_index = event_indices(&turn.events, |event| {
        event.get("type").and_then(Value::as_str) == Some("message.updated")
            && event
                .pointer("/properties/info/role")
                .and_then(Value::as_str)
                == Some("user")
    })
    .get(1)
    .copied();
    let first_assistant_completed = event_indices(&turn.events, |event| {
        event.get("type").and_then(Value::as_str) == Some("message.updated")
            && event
                .pointer("/properties/info/role")
                .and_then(Value::as_str)
                == Some("assistant")
            && event.pointer("/properties/info/time/completed").is_some()
    })
    .first()
    .copied();
    let idle_indices = event_indices(&turn.events, is_idle_event);

    println!("busy.sequence.second_user_event_index={second_user_index:?}");
    println!("busy.sequence.first_assistant_completed_index={first_assistant_completed:?}");
    println!("busy.sequence.idle_event_indices={idle_indices:?}");

    let caught_by_running_loop = matches!(
        (second_user_index, first_assistant_completed),
        (Some(user), Some(completed)) if user < completed
    ) && idle_indices
        .iter()
        .all(|idle| second_user_index.is_none_or(|user| *idle > user));

    if caught_by_running_loop {
        println!("SPIKE3_OUTCOME=被运行中的循环接住");
    } else if second_user_index.is_some_and(|user| idle_indices.iter().any(|idle| *idle < user)) {
        println!("SPIKE3_OUTCOME=排队");
    } else {
        println!("SPIKE3_OUTCOME=接受但时序无法完全分类");
    }
    Ok(())
}

fn event_indices(events: &[Value], predicate: impl Fn(&Value) -> bool) -> Vec<usize> {
    events
        .iter()
        .enumerate()
        .filter_map(move |(index, event)| predicate(event).then_some(index))
        .collect()
}

fn is_idle_event(event: &Value) -> bool {
    event.get("type").and_then(Value::as_str) == Some("session.idle")
        || (event.get("type").and_then(Value::as_str) == Some("session.status")
            && event
                .pointer("/properties/status/type")
                .and_then(Value::as_str)
                == Some("idle"))
}
