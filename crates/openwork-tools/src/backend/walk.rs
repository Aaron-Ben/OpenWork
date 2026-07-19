use std::io;
use std::path::PathBuf;

use ignore::WalkBuilder;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

const WALK_BUFFER_SIZE: usize = 64;

/// Incremental stream of files discovered by a filesystem traversal.
///
/// Dropping the stream closes its bounded receiver. The blocking producer then
/// stops at its next send instead of collecting the remainder of the tree.
pub struct FileWalk {
    receiver: mpsc::Receiver<io::Result<PathBuf>>,
    producer: Option<JoinHandle<io::Result<()>>>,
}

impl FileWalk {
    pub async fn next(&mut self) -> io::Result<Option<PathBuf>> {
        match self.receiver.recv().await {
            Some(path) => path.map(Some),
            None => {
                if let Some(producer) = self.producer.take() {
                    producer.await.map_err(io::Error::other)??;
                }
                Ok(None)
            }
        }
    }
}

pub(super) fn local_file_walk(root: PathBuf) -> FileWalk {
    let (sender, receiver) = mpsc::channel(WALK_BUFFER_SIZE);
    let producer = tokio::task::spawn_blocking(move || {
        for entry in WalkBuilder::new(root)
            .hidden(true)
            .git_ignore(true)
            .git_exclude(true)
            .build()
        {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    if sender
                        .blocking_send(Err(io::Error::other(error.to_string())))
                        .is_err()
                    {
                        return Ok(());
                    }
                    continue;
                }
            };
            if entry.file_type().is_some_and(|kind| kind.is_file())
                && sender.blocking_send(Ok(entry.into_path())).is_err()
            {
                return Ok(());
            }
        }
        Ok(())
    });

    FileWalk {
        receiver,
        producer: Some(producer),
    }
}
