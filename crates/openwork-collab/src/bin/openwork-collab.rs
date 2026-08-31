fn main() {
    let role = std::env::args().nth(1);
    let runtime = tokio::runtime::Runtime::new().expect("create Collaboration process runtime");
    let result = match role.as_deref() {
        Some("server") => runtime.block_on(openwork_collab::process::run_server_process()),
        Some("computer") => runtime.block_on(openwork_collab::process::run_computer_process()),
        _ => {
            eprintln!("Usage: openwork-collab <server|computer>");
            std::process::exit(2);
        }
    };
    if let Err(error) = result {
        eprintln!("Collaboration process failed: {error}");
        std::process::exit(1);
    }
}
