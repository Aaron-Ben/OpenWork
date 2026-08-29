mod auth;
pub mod control;
mod migration;
mod runtime;
mod storage;

use std::{net::SocketAddr, path::PathBuf};

use sqlx::PgPool;
use thiserror::Error;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use storage::CollaborationStore;

#[derive(Clone, Debug)]
pub struct ServerOptions {
    pub database_url: String,
    pub state_root: PathBuf,
    pub control_socket: PathBuf,
    pub runtime_bind: SocketAddr,
}

pub struct CollaborationServer;

pub struct ServerHandle {
    shutdown: CancellationToken,
    control_socket: PathBuf,
    pool: PgPool,
    tasks: Vec<JoinHandle<()>>,
    runtime_addr: SocketAddr,
}

impl CollaborationServer {
    pub async fn start(
        options: ServerOptions,
        shutdown: CancellationToken,
    ) -> Result<ServerHandle, ServerError> {
        if !options.runtime_bind.ip().is_loopback() {
            return Err(ServerError::NonLoopbackBind(options.runtime_bind));
        }
        tokio::fs::create_dir_all(&options.state_root).await?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(&options.state_root, std::fs::Permissions::from_mode(0o700))
                .await?;
        }

        let pool = PgPool::connect(&options.database_url).await?;
        migration::migrate(&pool).await?;
        let store = CollaborationStore::new(pool.clone());
        let signing_key = auth::load_or_create_signing_key(&options.state_root).await?;

        let control = control::bind(&options.control_socket).await?;
        let runtime = tokio::net::TcpListener::bind(options.runtime_bind).await?;
        let runtime_addr = runtime.local_addr()?;
        let runtime_base_url = format!("http://{runtime_addr}");

        let control_shutdown = shutdown.clone();
        let control_store = store.clone();
        let control_task = tokio::spawn(async move {
            control::serve(control, control_store, runtime_base_url, control_shutdown).await;
        });

        let runtime_shutdown = shutdown.clone();
        let runtime_task = tokio::spawn(async move {
            let app = runtime::router(store, signing_key);
            let _ = axum::serve(runtime, app)
                .with_graceful_shutdown(runtime_shutdown.cancelled_owned())
                .await;
        });

        Ok(ServerHandle {
            shutdown,
            control_socket: options.control_socket,
            pool,
            tasks: vec![control_task, runtime_task],
            runtime_addr,
        })
    }
}

impl ServerHandle {
    pub fn runtime_addr(&self) -> SocketAddr {
        self.runtime_addr
    }

    pub async fn shutdown(self) -> Result<(), ServerError> {
        self.shutdown.cancel();
        for task in self.tasks {
            task.await?;
        }
        self.pool.close().await;
        match tokio::fs::remove_file(&self.control_socket).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum ServerError {
    #[error("collaboration Runtime must bind to loopback, got {0}")]
    NonLoopbackBind(SocketAddr),
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("server task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
}
