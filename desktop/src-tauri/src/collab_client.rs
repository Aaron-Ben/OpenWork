use std::{
    fs::File,
    os::fd::AsRawFd,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};

use openwork_collab::{
    computer::sse::{SseDecoder, SseParseError},
    protocol::{
        request_id, ApiError, ComputerProcessBootstrap, ComputerProcessReady, DesktopCommand,
        DesktopCommandRequest, DesktopCommandResult, InvalidationEvent, RuntimeStatusView,
        ServerProcessBootstrap, ServerProcessReady, COLLAB_PROTOCOL_VERSION,
    },
    server::RuntimeCredentials,
};
use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
    sync::{broadcast, mpsc, oneshot, RwLock},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(20);
const COMMAND_ATTEMPTS: usize = 3;
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(250);
const COMPUTER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(20);
const SERVER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const SUPERVISOR_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub struct CollabDaemonClient {
    inner: Arc<ClientInner>,
}

struct ClientInner {
    _runtime_ownership: File,
    connection: RwLock<Option<DesktopConnection>>,
    supervisor: mpsc::Sender<SupervisorCommand>,
    invalidations: broadcast::Sender<InvalidationEvent>,
}

#[derive(Clone)]
struct DesktopConnection {
    http: reqwest::Client,
    base_url: String,
    runtime_session_id: String,
    desktop_secret: String,
}

enum SupervisorCommand {
    Shutdown(oneshot::Sender<()>),
}

struct ProcessGroup {
    server: Child,
    computer: Child,
    connection: DesktopConnection,
    runtime_root: PathBuf,
    sse_shutdown: CancellationToken,
    sse_task: JoinHandle<()>,
}

impl CollabDaemonClient {
    pub async fn discover_or_start() -> Result<Self, CollabClientError> {
        let state_root = state_root()?;
        secure_directory(&state_root).await?;
        let runtime_ownership = acquire_runtime_ownership(&state_root)?;
        let executable = std::env::current_exe()?;
        let (supervisor_tx, supervisor_rx) = mpsc::channel(1);
        let (invalidations, _) = broadcast::channel(128);
        let inner = Arc::new(ClientInner {
            _runtime_ownership: runtime_ownership,
            connection: RwLock::new(None),
            supervisor: supervisor_tx,
            invalidations,
        });
        let (ready_tx, ready_rx) = oneshot::channel();
        tokio::spawn(supervise(
            inner.clone(),
            state_root,
            executable,
            supervisor_rx,
            ready_tx,
        ));
        let startup = ready_rx
            .await
            .map_err(|_| CollabClientError::Startup("supervisor stopped".to_string()))?;
        startup.map_err(CollabClientError::Startup)?;
        Ok(Self { inner })
    }

    pub async fn call(
        &self,
        command: DesktopCommand,
    ) -> Result<DesktopCommandResult, CollabClientError> {
        let connection = self
            .inner
            .connection
            .read()
            .await
            .clone()
            .ok_or(CollabClientError::Unavailable)?;
        post_command(&connection, command).await
    }

    pub async fn shutdown(&self) {
        let (ack_tx, ack_rx) = oneshot::channel();
        if self
            .inner
            .supervisor
            .send(SupervisorCommand::Shutdown(ack_tx))
            .await
            .is_ok()
        {
            let _ = tokio::time::timeout(SUPERVISOR_SHUTDOWN_TIMEOUT, ack_rx).await;
        }
    }

    pub fn subscribe_invalidations(&self) -> broadcast::Receiver<InvalidationEvent> {
        self.inner.invalidations.subscribe()
    }
}

async fn supervise(
    inner: Arc<ClientInner>,
    state_root: PathBuf,
    executable: PathBuf,
    mut commands: mpsc::Receiver<SupervisorCommand>,
    first_ready: oneshot::Sender<Result<(), String>>,
) {
    let mut first_ready = Some(first_ready);
    loop {
        let mut group =
            match start_group(&state_root, &executable, inner.invalidations.clone()).await {
                Ok(group) => group,
                Err(error) => {
                    if let Some(ready) = first_ready.take() {
                        let _ = ready.send(Err(error.to_string()));
                        return;
                    }
                    tracing::error!(%error, "Collaboration process group restart failed");
                    tokio::select! {
                        command = commands.recv() => {
                            if let Some(SupervisorCommand::Shutdown(ack)) = command {
                                let _ = ack.send(());
                            }
                            return;
                        }
                        _ = tokio::time::sleep(Duration::from_secs(2)) => continue,
                    }
                }
            };
        *inner.connection.write().await = Some(group.connection.clone());
        if let Some(ready) = first_ready.take() {
            let _ = ready.send(Ok(()));
        }

        let shutdown_ack = loop {
            tokio::select! {
                command = commands.recv() => match command {
                    Some(SupervisorCommand::Shutdown(ack)) => break Some(ack),
                    None => break None,
                },
                _ = tokio::time::sleep(PROCESS_POLL_INTERVAL) => {
                    match group_exited(&mut group) {
                        Ok(true) => break None,
                        Ok(false) => {}
                        Err(error) => {
                            tracing::error!(%error, "Collaboration process status check failed");
                            break None;
                        }
                    }
                }
            }
        };

        *inner.connection.write().await = None;
        if let Err(error) = stop_group(&mut group).await {
            tracing::error!(%error, "Collaboration process group did not stop cleanly");
        }
        if let Err(error) = remove_runtime_root(&state_root, &group.runtime_root).await {
            tracing::warn!(%error, "Collaboration runtime directory cleanup failed");
        }
        if let Some(ack) = shutdown_ack {
            let _ = ack.send(());
            return;
        }
        tracing::warn!("Collaboration child exited; replacing the whole RuntimeSession");
    }
}

async fn start_group(
    state_root: &Path,
    executable: &Path,
    invalidations: broadcast::Sender<InvalidationEvent>,
) -> Result<ProcessGroup, CollabClientError> {
    let credentials = RuntimeCredentials::generate();
    let runtime_root = prepare_runtime_root(state_root, &credentials.runtime_session_id).await?;

    let server_bootstrap = ServerProcessBootstrap {
        runtime_session_id: credentials.runtime_session_id.clone(),
        desktop_secret: credentials.desktop_secret.clone(),
        computer_secret: credentials.computer_secret.clone(),
    };
    let (mut server, server_ready) = match spawn_child::<_, ServerProcessReady>(
        executable,
        "--openwork-collab-server",
        &server_bootstrap,
    )
    .await
    {
        Ok(value) => value,
        Err(error) => {
            let _ = remove_runtime_root(state_root, &runtime_root).await;
            return Err(error);
        }
    };
    if server_ready.protocol_version != COLLAB_PROTOCOL_VERSION
        || server_ready.runtime_session_id != credentials.runtime_session_id
        || !loopback_base_url(&server_ready.base_url)
    {
        let _ = stop_child(&mut server, SERVER_SHUTDOWN_TIMEOUT).await;
        let _ = remove_runtime_root(state_root, &runtime_root).await;
        return Err(CollabClientError::Protocol(
            "Server returned incompatible ready metadata".to_string(),
        ));
    }

    let computer_bootstrap = ComputerProcessBootstrap {
        runtime_session_id: credentials.runtime_session_id.clone(),
        base_url: server_ready.base_url.clone(),
        computer_secret: credentials.computer_secret,
        openwork_root: state_root.to_string_lossy().into_owned(),
        shim_executable: executable.to_string_lossy().into_owned(),
        engine_executable: std::env::var("OPENCODE_BIN").unwrap_or_else(|_| "opencode".to_string()),
    };
    let (mut computer, computer_ready) = match spawn_child::<_, ComputerProcessReady>(
        executable,
        "--openwork-collab-computer",
        &computer_bootstrap,
    )
    .await
    {
        Ok(value) => value,
        Err(error) => {
            let _ = stop_child(&mut server, SERVER_SHUTDOWN_TIMEOUT).await;
            let _ = remove_runtime_root(state_root, &runtime_root).await;
            return Err(error);
        }
    };
    if computer_ready.protocol_version != COLLAB_PROTOCOL_VERSION
        || computer_ready.runtime_session_id != credentials.runtime_session_id
    {
        let _ = stop_child(&mut computer, COMPUTER_SHUTDOWN_TIMEOUT).await;
        let _ = stop_child(&mut server, SERVER_SHUTDOWN_TIMEOUT).await;
        let _ = remove_runtime_root(state_root, &runtime_root).await;
        return Err(CollabClientError::Protocol(
            "Computer returned incompatible ready metadata".to_string(),
        ));
    }

    let connection = DesktopConnection {
        http: reqwest::Client::new(),
        base_url: server_ready.base_url,
        runtime_session_id: credentials.runtime_session_id,
        desktop_secret: credentials.desktop_secret,
    };
    if let Err(error) = wait_for_computer(&connection).await {
        let _ = stop_child(&mut computer, COMPUTER_SHUTDOWN_TIMEOUT).await;
        let _ = stop_child(&mut server, SERVER_SHUTDOWN_TIMEOUT).await;
        let _ = remove_runtime_root(state_root, &runtime_root).await;
        return Err(error);
    }
    let sse_shutdown = CancellationToken::new();
    let sse_task = tokio::spawn(desktop_sse_loop(
        connection.clone(),
        invalidations,
        sse_shutdown.clone(),
    ));
    Ok(ProcessGroup {
        server,
        computer,
        connection,
        runtime_root,
        sse_shutdown,
        sse_task,
    })
}

async fn spawn_child<B: Serialize, R: DeserializeOwned>(
    executable: &Path,
    role: &str,
    bootstrap: &B,
) -> Result<(Child, R), CollabClientError> {
    let mut command = Command::new(executable);
    command
        .arg(role)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    if role == "--openwork-collab-computer" {
        for name in [
            "DATABASE_URL",
            "REDIS_URL",
            "TEST_DATABASE_URL",
            "TEST_REDIS_URL",
            "PGHOST",
            "PGHOSTADDR",
            "PGPORT",
            "PGDATABASE",
            "PGUSER",
            "PGPASSWORD",
            "PGPASSFILE",
            "PGSERVICE",
            "PGSERVICEFILE",
        ] {
            command.env_remove(name);
        }
    }
    let mut child = command.spawn()?;
    let result = async {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| CollabClientError::Protocol("child stdin is unavailable".to_string()))?;
        let mut payload = serde_json::to_vec(bootstrap)?;
        payload.push(b'\n');
        stdin.write_all(&payload).await?;
        stdin.shutdown().await?;
        let stdout = child.stdout.take().ok_or_else(|| {
            CollabClientError::Protocol("child stdout is unavailable".to_string())
        })?;
        let mut line = String::new();
        let mut reader = BufReader::new(stdout).take(64 * 1024 + 1);
        let bytes = reader.read_line(&mut line).await?;
        if bytes == 0 || bytes > 64 * 1024 {
            return Err(CollabClientError::Protocol(
                "child ready response is missing or too large".to_string(),
            ));
        }
        serde_json::from_str(&line).map_err(Into::into)
    };
    match tokio::time::timeout(STARTUP_TIMEOUT, result).await {
        Ok(Ok(ready)) => Ok((child, ready)),
        Ok(Err(error)) => {
            let _ = stop_child(&mut child, SERVER_SHUTDOWN_TIMEOUT).await;
            Err(error)
        }
        Err(_) => {
            let _ = stop_child(&mut child, SERVER_SHUTDOWN_TIMEOUT).await;
            Err(CollabClientError::Startup(format!(
                "{role} did not become ready"
            )))
        }
    }
}

async fn wait_for_computer(connection: &DesktopConnection) -> Result<(), CollabClientError> {
    let deadline = tokio::time::Instant::now() + STARTUP_TIMEOUT;
    loop {
        match post_command(connection, DesktopCommand::Status).await {
            Ok(DesktopCommandResult::Status(RuntimeStatusView {
                runtime_session_id,
                last_computer_heartbeat: Some(_),
                ..
            })) if runtime_session_id == connection.runtime_session_id => return Ok(()),
            Ok(DesktopCommandResult::Status(_)) => {}
            Ok(_) => {
                return Err(CollabClientError::Protocol(
                    "Server returned a non-status startup response".to_string(),
                ));
            }
            Err(error) if tokio::time::Instant::now() < deadline => {
                tracing::debug!(%error, "waiting for Local Computer heartbeat");
            }
            Err(error) => return Err(error),
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(CollabClientError::Startup(
                "Local Computer did not report ready".to_string(),
            ));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn post_command(
    connection: &DesktopConnection,
    command: DesktopCommand,
) -> Result<DesktopCommandResult, CollabClientError> {
    let request = DesktopCommandRequest {
        request_id: command.is_mutating().then(request_id),
        command,
    };
    let mut delay = Duration::from_millis(100);
    for attempt in 0..COMMAND_ATTEMPTS {
        let response = connection
            .http
            .post(format!("{}/desktop/commands", connection.base_url))
            .bearer_auth(&connection.desktop_secret)
            .json(&request)
            .timeout(COMMAND_TIMEOUT)
            .send()
            .await;
        match response {
            Ok(response) if response.status().is_success() => match response.json().await {
                Ok(result) => return Ok(result),
                Err(error) if attempt + 1 == COMMAND_ATTEMPTS => return Err(error.into()),
                Err(_) => {}
            },
            Ok(response)
                if response.status().is_server_error() && attempt + 1 < COMMAND_ATTEMPTS => {}
            Ok(response) => {
                let status = response.status();
                let error = response.json::<ApiError>().await.unwrap_or(ApiError {
                    code: status.as_str().to_string(),
                    message: "Collaboration Server rejected the request".to_string(),
                });
                return Err(CollabClientError::Rejected {
                    code: error.code,
                    message: error.message,
                });
            }
            Err(error) if attempt + 1 == COMMAND_ATTEMPTS => return Err(error.into()),
            Err(_) => {}
        }
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(Duration::from_secs(1));
    }
    unreachable!("Desktop command retry loop returns on its final attempt")
}

async fn desktop_sse_loop(
    connection: DesktopConnection,
    invalidations: broadcast::Sender<InvalidationEvent>,
    shutdown: CancellationToken,
) {
    let mut backoff = Duration::from_secs(1);
    loop {
        let connected_at = tokio::time::Instant::now();
        if let Err(error) = desktop_sse_once(&connection, &invalidations, &shutdown).await {
            tracing::warn!(%error, "Desktop collaboration SSE disconnected");
        }
        if shutdown.is_cancelled() {
            return;
        }
        if connected_at.elapsed() >= Duration::from_secs(60) {
            backoff = Duration::from_secs(1);
        }
        tokio::select! {
            _ = shutdown.cancelled() => return,
            _ = tokio::time::sleep(backoff) => {}
        }
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

async fn desktop_sse_once(
    connection: &DesktopConnection,
    invalidations: &broadcast::Sender<InvalidationEvent>,
    shutdown: &CancellationToken,
) -> Result<(), CollabClientError> {
    let mut response = connection
        .http
        .get(format!("{}/desktop/events", connection.base_url))
        .bearer_auth(&connection.desktop_secret)
        .send()
        .await?
        .error_for_status()?;
    let mut decoder = SseDecoder::default();
    loop {
        let chunk = tokio::select! {
            _ = shutdown.cancelled() => return Ok(()),
            chunk = response.chunk() => chunk?,
        };
        let Some(chunk) = chunk else {
            return Ok(());
        };
        for event in decoder.push(&chunk)? {
            if event.event.as_deref() != Some("desktop") {
                continue;
            }
            let invalidation = serde_json::from_str::<InvalidationEvent>(&event.data)?;
            let _ = invalidations.send(invalidation);
        }
    }
}

fn group_exited(group: &mut ProcessGroup) -> Result<bool, std::io::Error> {
    Ok(group.server.try_wait()?.is_some() || group.computer.try_wait()?.is_some())
}

async fn stop_group(group: &mut ProcessGroup) -> Result<(), CollabClientError> {
    group.sse_shutdown.cancel();
    let computer = stop_child(&mut group.computer, COMPUTER_SHUTDOWN_TIMEOUT).await;
    let server = stop_child(&mut group.server, SERVER_SHUTDOWN_TIMEOUT).await;
    let _ = (&mut group.sse_task).await;
    computer?;
    server?;
    Ok(())
}

async fn stop_child(child: &mut Child, timeout: Duration) -> Result<(), std::io::Error> {
    if child.try_wait()?.is_some() {
        return Ok(());
    }
    if let Some(pid) = child.id() {
        let result = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
        if result != 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(error);
            }
        }
    }
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            child.kill().await?;
            let _ = child.wait().await?;
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn secure_directory(path: &Path) -> Result<(), std::io::Error> {
    tokio::fs::create_dir_all(path).await?;
    use std::os::unix::fs::PermissionsExt;
    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).await
}

fn acquire_runtime_ownership(state_root: &Path) -> Result<File, CollabClientError> {
    use std::os::unix::fs::OpenOptionsExt;

    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(state_root.join("runtime.lock"))?;
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result == 0 {
        Ok(file)
    } else {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::EWOULDBLOCK) {
            Err(CollabClientError::AlreadyRunning)
        } else {
            Err(error.into())
        }
    }
}

async fn prepare_runtime_root(
    state_root: &Path,
    runtime_session_id: &str,
) -> Result<PathBuf, std::io::Error> {
    let runtime_base = state_root.join("runtime");
    secure_directory(&runtime_base).await?;
    let mut entries = tokio::fs::read_dir(&runtime_base).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let file_type = entry.file_type().await?;
        if file_type.is_dir() && !file_type.is_symlink() {
            tokio::fs::remove_dir_all(path).await?;
        } else {
            tokio::fs::remove_file(path).await?;
        }
    }
    let runtime_root = runtime_base.join(runtime_session_id);
    secure_directory(&runtime_root).await?;
    Ok(runtime_root)
}

async fn remove_runtime_root(state_root: &Path, runtime_root: &Path) -> Result<(), std::io::Error> {
    if runtime_root.parent() != Some(state_root.join("runtime").as_path()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "refusing to remove an unexpected runtime directory",
        ));
    }
    match tokio::fs::remove_dir_all(runtime_root).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn loopback_base_url(value: &str) -> bool {
    value
        .strip_prefix("http://")
        .and_then(|address| address.parse::<std::net::SocketAddr>().ok())
        .is_some_and(|address| address.ip().is_loopback() && address.port() != 0)
}

fn state_root() -> Result<PathBuf, CollabClientError> {
    std::env::var_os("OPENWORK_COLLAB_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".openwork")))
        .ok_or(CollabClientError::MissingHome)
}

#[derive(Debug, Error)]
pub enum CollabClientError {
    #[error("collaboration Runtime could not start: {0}")]
    Startup(String),
    #[error("collaboration Runtime is restarting")]
    Unavailable,
    #[error("another OpenWork Desktop already owns the Collaboration Runtime")]
    AlreadyRunning,
    #[error("collaboration request was rejected ({code}): {message}")]
    Rejected { code: String, message: String },
    #[error("collaboration process protocol failed: {0}")]
    Protocol(String),
    #[error("HOME or OPENWORK_COLLAB_HOME must be set")]
    MissingHome,
    #[error("collaboration process failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("collaboration payload was invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("collaboration SSE was invalid: {0}")]
    Sse(#[from] SseParseError),
    #[error("collaboration HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
}

#[cfg(test)]
mod tests {
    use super::{
        acquire_runtime_ownership, loopback_base_url, prepare_runtime_root, CollabClientError,
    };

    #[test]
    fn only_random_loopback_http_endpoints_are_accepted() {
        assert!(loopback_base_url("http://127.0.0.1:43129"));
        assert!(loopback_base_url("http://[::1]:43129"));
        assert!(!loopback_base_url("http://0.0.0.0:43129"));
        assert!(!loopback_base_url("https://127.0.0.1:43129"));
        assert!(!loopback_base_url("http://127.0.0.1:0"));
    }

    #[tokio::test]
    async fn startup_removes_stale_runtime_directories_before_creating_the_current_one() {
        let directory = tempfile::tempdir().unwrap();
        let state_root = directory.path().join(".openwork");
        let stale = state_root.join("runtime/runtime-stale");
        tokio::fs::create_dir_all(&stale).await.unwrap();
        tokio::fs::write(stale.join("runtime-token"), b"obsolete")
            .await
            .unwrap();

        let current = prepare_runtime_root(&state_root, "runtime-current")
            .await
            .unwrap();

        assert_eq!(current, state_root.join("runtime/runtime-current"));
        assert!(current.is_dir());
        assert!(!stale.exists());
    }

    #[test]
    fn only_one_desktop_can_own_and_clean_the_runtime_directory() {
        let directory = tempfile::tempdir().unwrap();
        let first = acquire_runtime_ownership(directory.path()).unwrap();
        assert!(matches!(
            acquire_runtime_ownership(directory.path()),
            Err(CollabClientError::AlreadyRunning)
        ));
        drop(first);
        acquire_runtime_ownership(directory.path()).unwrap();
    }
}
