// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::args().any(|argument| argument == "--openwork-collab-daemon") {
        let runtime = tokio::runtime::Runtime::new().expect("create collaboration daemon runtime");
        if let Err(error) = runtime.block_on(async {
            openwork_collab::daemon::run(openwork_collab::daemon::DaemonConfig::from_env()?).await
        }) {
            eprintln!("collaboration daemon failed: {error}");
            std::process::exit(1);
        }
        return;
    }
    openwork_desktop_lib::run()
}
