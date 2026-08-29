use std::{path::PathBuf, process::Stdio, time::Duration};

use openwork_collab::{
    protocol::{ControlRequest, ControlResponse, COLLAB_PROTOCOL_VERSION},
    server::control::{request, ControlError},
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct CollabDaemonClient {
    socket_path: PathBuf,
}

impl CollabDaemonClient {
    pub async fn discover_or_start() -> Result<Self, CollabClientError> {
        let state_root = state_root()?;
        let client = Self {
            socket_path: state_root.join("server/control.sock"),
        };
        if client.call(&ControlRequest::ListAgents).await.is_err() {
            launch_current_executable("--openwork-collab-server")?;
            client.wait_until_ready().await?;
        }
        let registration = match client.call(&ControlRequest::EnsureLocalComputer).await? {
            ControlResponse::LocalComputer(registration) => registration,
            ControlResponse::Error { message } => return Err(CollabClientError::Rejected(message)),
            _ => {
                return Err(CollabClientError::Protocol(
                    "unexpected ensure response".to_string(),
                ))
            }
        };
        let identity_path = state_root.join("computer/computer.json");
        let device_token = match registration.device_token {
            Some(token) => token,
            None => read_identity(&identity_path)?.device_token,
        };
        write_identity(
            &identity_path,
            &ComputerIdentity {
                protocol_version: COLLAB_PROTOCOL_VERSION,
                computer_id: "local".to_string(),
                runtime_base_url: registration.runtime_base_url,
                device_token,
            },
        )?;
        launch_current_executable("--openwork-collab-computer")?;
        Ok(client)
    }

    pub async fn call(
        &self,
        control_request: &ControlRequest,
    ) -> Result<ControlResponse, CollabClientError> {
        request(&self.socket_path, control_request)
            .await
            .map_err(Into::into)
    }

    async fn wait_until_ready(&self) -> Result<(), CollabClientError> {
        let mut last_error = None;
        for _ in 0..100 {
            match self.call(&ControlRequest::ListAgents).await {
                Ok(_) => return Ok(()),
                Err(error) => last_error = Some(error.to_string()),
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(CollabClientError::Startup(last_error.unwrap_or_else(
            || "Server did not create its control socket".to_string(),
        )))
    }
}

fn launch_current_executable(role: &str) -> Result<(), CollabClientError> {
    let executable = std::env::current_exe()?;
    let mut command = Command::new(executable);
    command
        .arg(role)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(false)
        .process_group(0);
    command.spawn()?;
    Ok(())
}

fn state_root() -> Result<PathBuf, CollabClientError> {
    std::env::var_os("OPENWORK_COLLAB_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".openwork")))
        .ok_or(CollabClientError::MissingHome)
}

fn read_identity(path: &std::path::Path) -> Result<ComputerIdentity, CollabClientError> {
    let bytes = std::fs::read(path).map_err(|error| {
        CollabClientError::Identity(format!(
            "{} could not be read after the one-time device token was consumed: {error}",
            path.display()
        ))
    })?;
    serde_json::from_slice(&bytes).map_err(|error| CollabClientError::Identity(error.to_string()))
}

fn write_identity(
    path: &std::path::Path,
    identity: &ComputerIdentity,
) -> Result<(), CollabClientError> {
    use std::os::unix::fs::PermissionsExt;
    let parent = path.parent().expect("computer identity has parent");
    std::fs::create_dir_all(parent)?;
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    let temporary = parent.join(format!(".computer-{}.tmp", std::process::id()));
    std::fs::write(&temporary, serde_json::to_vec_pretty(identity)?)?;
    std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600))?;
    std::fs::rename(temporary, path)?;
    Ok(())
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct ComputerIdentity {
    pub protocol_version: u32,
    pub computer_id: String,
    pub runtime_base_url: String,
    pub device_token: String,
}

#[derive(Debug, Error)]
pub enum CollabClientError {
    #[error("collaboration Server could not start: {0}")]
    Startup(String),
    #[error("collaboration control request was rejected: {0}")]
    Rejected(String),
    #[error("collaboration control protocol failed: {0}")]
    Protocol(String),
    #[error("Local Computer identity is unavailable: {0}")]
    Identity(String),
    #[error("HOME or OPENWORK_COLLAB_HOME must be set")]
    MissingHome,
    #[error(transparent)]
    Control(#[from] ControlError),
    #[error("collaboration process failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Local Computer identity could not be encoded: {0}")]
    Json(#[from] serde_json::Error),
}
