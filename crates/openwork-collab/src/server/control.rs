use std::path::Path;

use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{UnixListener, UnixStream},
};
use tokio_util::sync::CancellationToken;

use crate::protocol::{ControlRequest, ControlResponse};

use super::{scheduler::Scheduler, storage::CollaborationStore};

const MAX_FRAME_BYTES: usize = 1024 * 1024;

pub async fn bind(path: &Path) -> Result<UnixListener, std::io::Error> {
    if tokio::fs::try_exists(path).await? {
        if UnixStream::connect(path).await.is_ok() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                format!("a collaboration Server already owns {}", path.display()),
            ));
        }
        tokio::fs::remove_file(path).await?;
    }
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let listener = UnixListener::bind(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).await?;
    }
    Ok(listener)
}

pub async fn serve(
    listener: UnixListener,
    store: CollaborationStore,
    scheduler: Scheduler,
    runtime_base_url: String,
    shutdown: CancellationToken,
) {
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => break,
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else { break };
                let store = store.clone();
                let scheduler = scheduler.clone();
                let runtime_base_url = runtime_base_url.clone();
                let connection_shutdown = shutdown.clone();
                tokio::spawn(async move {
                    let _ = handle(
                        stream,
                        store,
                        scheduler,
                        runtime_base_url,
                        connection_shutdown,
                    )
                    .await;
                });
            }
        }
    }
}

async fn handle(
    mut stream: UnixStream,
    store: CollaborationStore,
    scheduler: Scheduler,
    runtime_base_url: String,
    shutdown: CancellationToken,
) -> Result<(), ControlError> {
    let request: ControlRequest = read_frame(&mut stream).await?;
    let shutdown_requested = matches!(&request, ControlRequest::ShutdownServer);
    let response = match request {
        ControlRequest::EnsureLocalComputer => store
            .ensure_local_computer(&runtime_base_url)
            .await
            .map(ControlResponse::LocalComputer)
            .map_err(|error| error.to_string()),
        ControlRequest::CreateAgent {
            id,
            display_name,
            system_prompt,
            model,
        } => store
            .create_agent(&id, &display_name, &system_prompt, &model)
            .await
            .map(ControlResponse::Agent)
            .map_err(|error| error.to_string()),
        ControlRequest::ListAgents => store
            .list_agents()
            .await
            .map(|agents| ControlResponse::Agents { agents })
            .map_err(|error| error.to_string()),
        ControlRequest::ListRooms => store
            .list_rooms()
            .await
            .map(|rooms| ControlResponse::Rooms { rooms })
            .map_err(|error| error.to_string()),
        ControlRequest::CreateDirectRoom { agent_id } => store
            .create_direct_room(&agent_id)
            .await
            .map(ControlResponse::Room)
            .map_err(|error| error.to_string()),
        ControlRequest::SendMessage { room_id, body } => {
            match store.send_user_message(&room_id, &body).await {
                Ok(message) => {
                    scheduler
                        .message_committed(&message.id, &message.room_id, &message.author_id)
                        .await;
                    Ok(ControlResponse::Message(message))
                }
                Err(error) => Err(error.to_string()),
            }
        }
        ControlRequest::ListMessages { room_id } => store
            .list_messages(&room_id)
            .await
            .map(|messages| ControlResponse::Messages { messages })
            .map_err(|error| error.to_string()),
        ControlRequest::ShutdownServer => Ok(ControlResponse::Acknowledged),
    };
    let response = response.unwrap_or_else(|message| ControlResponse::Error { message });
    write_frame(&mut stream, &response).await?;
    if shutdown_requested {
        shutdown.cancel();
    }
    Ok(())
}

pub async fn request(
    socket: &Path,
    request: &ControlRequest,
) -> Result<ControlResponse, ControlError> {
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        request_without_timeout(socket, request),
    )
    .await
    .map_err(|_| ControlError::Timeout)?
}

async fn request_without_timeout(
    socket: &Path,
    request: &ControlRequest,
) -> Result<ControlResponse, ControlError> {
    let mut stream = UnixStream::connect(socket).await?;
    write_frame(&mut stream, request).await?;
    read_frame(&mut stream).await
}

async fn write_frame<T: serde::Serialize>(
    writer: &mut (impl AsyncWrite + Unpin),
    value: &T,
) -> Result<(), ControlError> {
    let body = serde_json::to_vec(value)?;
    if body.len() > MAX_FRAME_BYTES {
        return Err(ControlError::FrameTooLarge(body.len()));
    }
    writer.write_u32(body.len() as u32).await?;
    writer.write_all(&body).await?;
    writer.flush().await?;
    Ok(())
}

async fn read_frame<T: serde::de::DeserializeOwned>(
    reader: &mut (impl AsyncRead + Unpin),
) -> Result<T, ControlError> {
    let length = reader.read_u32().await? as usize;
    if length > MAX_FRAME_BYTES {
        return Err(ControlError::FrameTooLarge(length));
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).await?;
    Ok(serde_json::from_slice(&body)?)
}

#[derive(Debug, thiserror::Error)]
pub enum ControlError {
    #[error("control socket I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid control message: {0}")]
    Json(#[from] serde_json::Error),
    #[error("control frame is too large: {0} bytes")]
    FrameTooLarge(usize),
    #[error("control request timed out")]
    Timeout,
}
