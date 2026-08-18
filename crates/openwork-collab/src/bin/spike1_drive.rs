use std::time::Duration;

use openwork_collab::{
    OpenCodeClient, OpenCodeServer, SpikeResult, collect_until_idle, message_error,
    print_turn_summary, session_id,
};

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

    let health = client.health().await?;
    println!("health={health}");

    let v1_probe = client.probe_path("/session?limit=1").await?;
    v1_probe.print("path_probe.v1_session");
    let v2_probe = client.probe_path("/api/session").await?;
    v2_probe.print("path_probe.v2_api_session");

    let session = client.create_session("OpenWork P0 spike 1").await?;
    println!("session.create.response={session}");
    let id = session_id(&session)?.to_string();
    println!("session.id={id}");
    println!("selected_path_family=/session/* (non-experimental v1)");

    let mut events = client.event_stream().await?;
    let prompt = "Reply with exactly P0_DRIVE_OK and no other text.";
    let accepted = client.prompt_async(&id, prompt).await?;
    accepted.print("prompt_async");
    if accepted.status != reqwest::StatusCode::NO_CONTENT {
        return Err(message_error("prompt_async was not accepted with HTTP 204"));
    }

    let turn = collect_until_idle(&mut events, &id, Duration::from_secs(180)).await?;
    print_turn_summary("turn", &turn);
    if turn.final_text().trim() != "P0_DRIVE_OK" {
        return Err(message_error(format!(
            "unexpected final assistant text: {:?}",
            turn.final_text()
        )));
    }
    if turn.usage().is_none() {
        return Err(message_error("no final token usage arrived over SSE"));
    }

    println!("SPIKE1_RESULT=成立");
    Ok(())
}
