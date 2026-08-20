use std::{path::PathBuf, process::Stdio, time::Duration};

use openwork_collab::daemon::{request, DaemonConfig, IpcRequest, COLLAB_PROTOCOL_VERSION};
use serde::{de::DeserializeOwned, Deserialize};
use thiserror::Error;
use tokio::{net::UnixStream, process::Command};

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
        client
            .ensure_running(Self::launch_current_executable)
            .await?;
        Ok(client)
    }

    fn launch_current_executable() -> Result<(), CollabClientError> {
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
        Ok(())
    }

    async fn ensure_running(
        &self,
        launch: impl FnOnce() -> Result<(), CollabClientError>,
    ) -> Result<(), CollabClientError> {
        match self.handshake().await {
            Ok(handshake) if handshake.is_current() => return Ok(()),
            Ok(_) => {
                let _: serde_json::Value = self.call(&IpcRequest::Shutdown).await?;
                self.wait_until_stopped().await?;
            }
            Err(_) => {}
        }

        launch()?;

        let mut last_error = None;
        for _ in 0..100 {
            match self.handshake().await {
                Ok(handshake) if handshake.is_current() => return Ok(()),
                Ok(handshake) => {
                    last_error = Some(format!(
                        "daemon protocol {} does not match Desktop protocol {COLLAB_PROTOCOL_VERSION}",
                        handshake.protocol_version,
                    ));
                }
                Err(error) => last_error = Some(error.to_string()),
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(CollabClientError::Startup(last_error.unwrap_or_else(
            || "daemon did not create its socket".to_string(),
        )))
    }

    async fn wait_until_stopped(&self) -> Result<(), CollabClientError> {
        for _ in 0..100 {
            if UnixStream::connect(&self.socket_path).await.is_err() {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(CollabClientError::Startup(format!(
            "incompatible collaboration daemon did not stop at {}",
            self.socket_path.display(),
        )))
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

    async fn handshake(&self) -> Result<DaemonHandshake, CollabClientError> {
        self.call(&IpcRequest::Ping).await
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DaemonHandshake {
    pong: bool,
    #[serde(default)]
    protocol_version: u32,
}

impl DaemonHandshake {
    fn is_current(&self) -> bool {
        self.pong && self.protocol_version == COLLAB_PROTOCOL_VERSION
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

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use openwork_collab::daemon::IpcRequest;
    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        net::UnixListener,
    };

    use super::{CollabDaemonClient, COLLAB_PROTOCOL_VERSION};

    fn socket_path() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "openwork-collab-client-{}-{nonce}.sock",
            std::process::id()
        ))
    }

    async fn reply(listener: &UnixListener, response: &[u8]) -> IpcRequest {
        let (mut stream, _) = listener.accept().await.expect("accept request");
        let mut request = String::new();
        BufReader::new(&mut stream)
            .read_line(&mut request)
            .await
            .expect("read request");
        stream.write_all(response).await.expect("write response");
        serde_json::from_str(&request).expect("parse request")
    }

    async fn serve_incompatible_daemon(listener: UnixListener, path: PathBuf) -> [IpcRequest; 2] {
        let ping = reply(
            &listener,
            b"{\"ok\":true,\"data\":{\"pong\":true,\"protocolVersion\":0},\"error\":null}\n",
        )
        .await;
        let shutdown = reply(
            &listener,
            b"{\"ok\":true,\"data\":{\"shuttingDown\":true},\"error\":null}\n",
        )
        .await;
        drop(listener);
        std::fs::remove_file(path).expect("remove incompatible socket");
        [ping, shutdown]
    }

    #[tokio::test]
    async fn replaces_a_daemon_when_its_protocol_is_incompatible() {
        let socket_path = socket_path();
        let incompatible_listener =
            UnixListener::bind(&socket_path).expect("bind incompatible daemon");
        let incompatible_server = tokio::spawn(serve_incompatible_daemon(
            incompatible_listener,
            socket_path.clone(),
        ));
        let client = CollabDaemonClient {
            socket_path: socket_path.clone(),
        };
        let launch_path = socket_path.clone();

        let requests = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            client
                .ensure_running(move || {
                    let listener = UnixListener::bind(&launch_path)?;
                    tokio::spawn(async move {
                        let response = format!(
                            "{{\"ok\":true,\"data\":{{\"pong\":true,\"protocolVersion\":{COLLAB_PROTOCOL_VERSION}}},\"error\":null}}\n"
                        );
                        reply(&listener, response.as_bytes()).await;
                    });
                    Ok(())
                })
                .await
                .expect("replace incompatible daemon");
            incompatible_server
                .await
                .expect("incompatible daemon stopped")
        })
        .await
        .expect("replacement timed out");

        let [ping, shutdown] = requests;
        assert!(matches!(ping, IpcRequest::Ping));
        assert!(matches!(shutdown, IpcRequest::Shutdown));
        std::fs::remove_file(socket_path).expect("remove replacement socket");
    }
}
