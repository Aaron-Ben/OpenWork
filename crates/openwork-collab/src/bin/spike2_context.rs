use std::time::Duration;

use openwork_collab::{
    OpenCodeClient, OpenCodeServer, SpikeResult, collect_until_idle, message_error,
    print_turn_summary, session_id,
};

const FACT: &str = "P0_CONTEXT_FACT_7Q9M2";

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
    let session = client.create_session("OpenWork P0 spike 2").await?;
    println!("session.create.response={session}");
    let id = session_id(&session)?.to_string();
    println!("session.id.from=POST /session response field `id`");
    println!("session.id={id}");
    let mut events = client.event_stream().await?;

    let first_prompt =
        format!("Remember this exact fact for my next message: {FACT}. Reply with exactly STORED.");
    let first_response = client.prompt_async(&id, &first_prompt).await?;
    first_response.print("round1.prompt_async");
    if first_response.status != reqwest::StatusCode::NO_CONTENT {
        return Err(message_error("round 1 prompt_async was not accepted"));
    }
    let first = collect_until_idle(&mut events, &id, Duration::from_secs(180)).await?;
    print_turn_summary("round1", &first);

    println!("session.id.reused={id}");
    let second_prompt =
        "What exact fact did I ask you to remember? Reply with the fact only, byte for byte.";
    let second_response = client.prompt_async(&id, second_prompt).await?;
    second_response.print("round2.prompt_async");
    if second_response.status != reqwest::StatusCode::NO_CONTENT {
        return Err(message_error("round 2 prompt_async was not accepted"));
    }
    let second = collect_until_idle(&mut events, &id, Duration::from_secs(180)).await?;
    print_turn_summary("round2", &second);

    if !second.final_text().contains(FACT) {
        return Err(message_error(format!(
            "second answer did not contain remembered fact {FACT:?}: {:?}",
            second.final_text()
        )));
    }
    println!("SPIKE2_RESULT=成立");
    Ok(())
}
