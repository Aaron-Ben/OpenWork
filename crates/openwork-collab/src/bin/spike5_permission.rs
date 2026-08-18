use std::time::Duration;

use openwork_collab::{
    OpenCodeClient, OpenCodeServer, SpikeResult, SseStream, TurnEvidence, event_session_id,
    message_error, print_turn_summary, session_id,
};
use serde_json::Value;
use tokio::time::Instant;

const FIRST_SECRET: &str = "P0_EXTERNAL_ONCE_9F3L";
const SECOND_SECRET: &str = "P0_EXTERNAL_REJECT_6W2R";
const REJECT_MESSAGE: &str = "P0_REJECT_REASON_DO_NOT_READ";

#[tokio::main]
async fn main() -> SpikeResult<()> {
    let home = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    let once_file = outside.path().join("once.txt");
    let reject_file = outside.path().join("reject.txt");
    tokio::fs::write(&once_file, FIRST_SECRET).await?;
    tokio::fs::write(&reject_file, SECOND_SECRET).await?;
    println!("home.directory={}", home.path().display());
    println!("external.once_file={}", once_file.display());
    println!("external.reject_file={}", reject_file.display());

    let mut server = OpenCodeServer::start().await?;
    let result = run(server.base_url(), home.path(), &once_file, &reject_file).await;
    server.shutdown().await;
    result
}

async fn run(
    base_url: &str,
    home: &std::path::Path,
    once_file: &std::path::Path,
    reject_file: &std::path::Path,
) -> SpikeResult<()> {
    let client = OpenCodeClient::new(base_url, home);
    let session = client.create_session("OpenWork P0 spike 5").await?;
    println!("session.create.response={session}");
    let id = session_id(&session)?.to_string();
    let mut events = client.event_stream().await?;

    let once_prompt = format!(
        "Use the read tool to read the exact file path {}. After it succeeds, reply with the file contents only.",
        once_file.display()
    );
    let accepted = client.prompt_async(&id, &once_prompt).await?;
    accepted.print("once.prompt_async");
    if accepted.status != reqwest::StatusCode::NO_CONTENT {
        return Err(message_error("once prompt_async was not accepted"));
    }
    let (mut once_turn, once_permission) =
        wait_for_permission(&mut events, &id, Duration::from_secs(120)).await?;
    println!("once.permission.asked={once_permission}");
    let once_permission_id = permission_id(&once_permission)?.to_string();
    let once_reply = client
        .reply_permission(&once_permission_id, "once", None)
        .await?;
    once_reply.print("once.permission.reply");
    if !once_reply.status.is_success() {
        return Err(message_error("permission once reply failed"));
    }
    collect_remaining_turn(&mut events, &id, &mut once_turn, Duration::from_secs(180)).await?;
    print_turn_summary("once.turn", &once_turn);
    if !once_turn.final_text().contains(FIRST_SECRET) {
        return Err(message_error(format!(
            "Agent did not continue with external file contents after once: {:?}",
            once_turn.final_text()
        )));
    }

    let reject_prompt = format!(
        "Use the read tool to read the exact file path {}. If permission is rejected with a user message, repeat that user message exactly and do not try another way to read the file.",
        reject_file.display()
    );
    let accepted = client.prompt_async(&id, &reject_prompt).await?;
    accepted.print("reject.prompt_async");
    if accepted.status != reqwest::StatusCode::NO_CONTENT {
        return Err(message_error("reject prompt_async was not accepted"));
    }
    let (mut reject_turn, reject_permission) =
        wait_for_permission(&mut events, &id, Duration::from_secs(120)).await?;
    println!("reject.permission.asked={reject_permission}");
    let reject_permission_id = permission_id(&reject_permission)?.to_string();
    let reject_reply = client
        .reply_permission(&reject_permission_id, "reject", Some(REJECT_MESSAGE))
        .await?;
    reject_reply.print("reject.permission.reply");
    if !reject_reply.status.is_success() {
        return Err(message_error("permission reject reply failed"));
    }
    collect_remaining_turn(&mut events, &id, &mut reject_turn, Duration::from_secs(180)).await?;
    print_turn_summary("reject.turn", &reject_turn);
    if !reject_turn.final_text().contains(REJECT_MESSAGE) {
        return Err(message_error(format!(
            "model answer did not demonstrate visibility of reject message {REJECT_MESSAGE:?}: {:?}",
            reject_turn.final_text()
        )));
    }
    if reject_turn.final_text().contains(SECOND_SECRET) {
        return Err(message_error(
            "rejected external file contents unexpectedly appeared in the answer",
        ));
    }

    println!("SPIKE5_RESULT=成立");
    Ok(())
}

async fn wait_for_permission(
    events: &mut SseStream,
    session_id: &str,
    max_wait: Duration,
) -> SpikeResult<(TurnEvidence, Value)> {
    let deadline = Instant::now() + max_wait;
    let mut turn = TurnEvidence::default();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(message_error("timed out waiting for permission.asked"));
        }
        let event = events.next_json(remaining).await?;
        let asked = event_session_id(&event) == Some(session_id)
            && event.get("type").and_then(Value::as_str) == Some("permission.asked")
            && event
                .pointer("/properties/permission")
                .and_then(Value::as_str)
                == Some("external_directory");
        turn.observe(session_id, event.clone());
        if asked {
            return Ok((turn, event));
        }
        if turn.saw_idle {
            return Err(message_error(
                "session became idle without emitting permission.asked",
            ));
        }
    }
}

async fn collect_remaining_turn(
    events: &mut SseStream,
    session_id: &str,
    turn: &mut TurnEvidence,
    max_wait: Duration,
) -> SpikeResult<()> {
    let deadline = Instant::now() + max_wait;
    while !turn.saw_idle {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(message_error(
                "session did not become idle after permission reply",
            ));
        }
        let event = events.next_json(remaining).await?;
        turn.observe(session_id, event);
    }
    Ok(())
}

fn permission_id(event: &Value) -> SpikeResult<&str> {
    event
        .pointer("/properties/id")
        .and_then(Value::as_str)
        .ok_or_else(|| message_error(format!("permission.asked has no id: {event}")))
}
