use std::path::PathBuf;

fn main() {
    let invoked_name = std::env::args_os()
        .next()
        .and_then(|path| PathBuf::from(path).file_name().map(|name| name.to_owned()));
    if invoked_name.as_deref() == Some(std::ffi::OsStr::new("openwork")) {
        std::process::exit(openwork_collab::computer::shim::main());
    }
    let role = std::env::args().nth(1);
    if role.as_deref() == Some("--openwork-collab-server") {
        let _ = dotenvy::dotenv();
        run_role(openwork_collab::process::run_server_process());
        return;
    }
    if role.as_deref() == Some("--openwork-collab-computer") {
        run_role(openwork_collab::process::run_computer_process());
        return;
    }
    openwork_desktop_lib::run()
}

fn run_role<E>(future: impl std::future::Future<Output = Result<(), E>>)
where
    E: std::fmt::Display,
{
    let runtime = tokio::runtime::Runtime::new().expect("create local collaboration runtime");
    if let Err(error) = runtime.block_on(future) {
        eprintln!("local collaboration process failed: {error}");
        std::process::exit(1);
    }
}
