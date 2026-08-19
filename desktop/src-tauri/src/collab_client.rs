use std::{path::PathBuf, process::Stdio, time::Duration};

use openwork_collab::daemon::{request, DaemonConfig, IpcRequest};
use serde::de::DeserializeOwned;
use thiserror::Error;
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct CollabDaemonClient {
    socket_path: PathBuf,
}

impl CollabDaemonClient {
    pub async fn discover_or_start() -> Result<Self, CollabClientError> {
        let config = DaemonConfig::from_env()?;
        let client = Self {
            socket_path: config.socket_path(),
        };
        if client.ping().await.is_ok() {
            return Ok(client);
        }

        let executable = std::env::current_exe()?;
        let mut command = Command::new(executable);
        command
            .arg("--openwork-collab-daemon")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(false);
        #[cfg(unix)]
        command.process_group(0);
        command.spawn()?;

        let mut last_error = None;
        for _ in 0..100 {
            match client.ping().await {
                Ok(()) => return Ok(client),
                Err(error) => last_error = Some(error),
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(CollabClientError::Startup(
            last_error
                .map(|error| error.to_string())
                .unwrap_or_else(|| "daemon did not create its socket".to_string()),
        ))
    }

    pub fn socket_path(&self) -> &std::path::Path {
        &self.socket_path
    }

    pub async fn call<T: DeserializeOwned>(
        &self,
        ipc: &IpcRequest,
    ) -> Result<T, CollabClientError> {
        let response = request(&self.socket_path, ipc).await?;
        if !response.ok {
            return Err(CollabClientError::Rejected(
                response
                    .error
                    .unwrap_or_else(|| "daemon rejected the request".to_string()),
            ));
        }
        let data = response
            .data
            .ok_or_else(|| CollabClientError::Protocol("response data is missing".to_string()))?;
        serde_json::from_value(data).map_err(CollabClientError::Json)
    }

    async fn ping(&self) -> Result<(), CollabClientError> {
        let _: serde_json::Value = self.call(&IpcRequest::Ping).await?;
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum CollabClientError {
    #[error(transparent)]
    Daemon(#[from] openwork_collab::daemon::DaemonError),
    #[error("collaboration daemon could not start: {0}")]
    Startup(String),
    #[error("collaboration daemon rejected the request: {0}")]
    Rejected(String),
    #[error("collaboration daemon protocol failed: {0}")]
    Protocol(String),
    #[error("collaboration daemon response was invalid: {0}")]
    Json(serde_json::Error),
    #[error("collaboration daemon process failed: {0}")]
    Io(#[from] std::io::Error),
}
