use std::{
    future::Future,
    pin::Pin,
    sync::mpsc,
    task::{Context, Poll},
};

use futures_util::Stream;
#[allow(deprecated)]
use openwork_protocol::model::{ModelError, ModelEvent, ModelResponse, ModelStream};
use tokio::sync::mpsc as tokio_mpsc;

struct ChannelModelStream {
    receiver: tokio_mpsc::Receiver<Result<ModelEvent, ModelError>>,
    producer: tokio::task::JoinHandle<()>,
}

impl Stream for ChannelModelStream {
    type Item = Result<ModelEvent, ModelError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.receiver.poll_recv(cx)
    }
}

impl Drop for ChannelModelStream {
    fn drop(&mut self) {
        // Dropping the public stream is the cancellation boundary: aborting the producer drops
        // the in-flight reqwest future (and, for the retry wrapper, the current inner stream).
        self.producer.abort();
    }
}

pub(crate) type EventCallback = Box<dyn FnMut(ModelEvent) + Send>;

pub(crate) fn model_stream_from_callback<F, Fut>(run: F) -> ModelStream
where
    F: FnOnce(EventCallback) -> Fut + Send + 'static,
    Fut: Future<Output = Result<ModelResponse, ModelError>> + Send + 'static,
{
    const CAPACITY: usize = 32;
    let (sync_sender, sync_receiver) = mpsc::sync_channel(CAPACITY);
    let (async_sender, async_receiver) = tokio_mpsc::channel(CAPACITY);

    tokio::task::spawn_blocking(move || {
        while let Ok(item) = sync_receiver.recv() {
            if async_sender.blocking_send(item).is_err() {
                break;
            }
        }
    });

    let producer = tokio::spawn(async move {
        let event_sender = sync_sender.clone();
        let callback: EventCallback = Box::new(move |event| {
            let _ = event_sender.send(Ok(event));
        });
        let item = match run(callback).await {
            Ok(response) => Ok(ModelEvent::ResponseCompleted {
                response: Box::new(response),
            }),
            Err(error) => Err(error),
        };
        let _ = sync_sender.send(item);
    });

    Box::pin(ChannelModelStream {
        receiver: async_receiver,
        producer,
    })
}
