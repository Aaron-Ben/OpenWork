use std::pin::Pin;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::task::{Context, Poll};
use std::time::Duration;

use async_trait::async_trait;
use futures_util::{Stream, StreamExt, stream};
use openwork_protocol::model::{
    FinishReason, ModelCallOptions, ModelError, ModelErrorCode, ModelEvent, ModelPort,
    ModelRequest, ModelResponse, ModelStream, ModelTransportObserver, ModelTransportSignal,
    ModelTransportSignalKind, RetryHint,
};
use openwork_providers::{RetryDecision, RetryPolicy, RetryingModelPort};

fn request() -> ModelRequest {
    ModelRequest {
        model: "test-model".to_string(),
        messages: Vec::new(),
        temperature: None,
        max_output_tokens: None,
        thinking: None,
        tools: Vec::new(),
    }
}

fn response() -> ModelResponse {
    ModelResponse {
        response_id: None,
        provider_request_id: None,
        model: Some("test-model".to_string()),
        text: "ok".to_string(),
        reasoning_text: None,
        tool_calls: Vec::new(),
        provider_opaque_blocks: Vec::new(),
        finish_reason: FinishReason::Stop,
        raw_finish_reason: Some("stop".to_string()),
        usage: None,
    }
}

fn rate_limit_error() -> ModelError {
    ModelError::http(
        ModelErrorCode::RateLimited,
        429,
        "slow down",
        Some("rate_limit".to_string()),
        None,
        RetryHint::AfterMillis(0),
    )
}

#[test]
fn retry_policy_honors_retry_hint_and_stream_gate() {
    let policy = RetryPolicy::new(3, Duration::ZERO, Duration::from_secs(1)).with_jitter(false);

    assert_eq!(
        policy.decision(1, &rate_limit_error(), false),
        RetryDecision::RetryAfter(Duration::ZERO)
    );
    assert_eq!(
        policy.decision(1, &rate_limit_error(), true),
        RetryDecision::DoNotRetry
    );
    assert_eq!(
        policy.decision(1, &ModelError::invalid_request("bad request"), false),
        RetryDecision::DoNotRetry
    );
}

#[test]
fn retry_after_above_limit_is_rejected_instead_of_shortened() {
    let policy = RetryPolicy::new(3, Duration::from_millis(1), Duration::from_secs(1))
        .with_jitter(false)
        .with_max_retry_after(Duration::from_secs(30));
    let error = ModelError::http(
        ModelErrorCode::RateLimited,
        429,
        "slow down",
        None,
        None,
        RetryHint::AfterMillis(120_000),
    );

    assert_eq!(policy.decision(1, &error, false), RetryDecision::DoNotRetry);
}

struct FlakyProvider {
    attempts: AtomicUsize,
}

#[async_trait]
impl ModelPort for FlakyProvider {
    async fn invoke(
        &self,
        _request: ModelRequest,
        _options: ModelCallOptions,
    ) -> Result<ModelStream, ModelError> {
        let attempt = self.attempts.fetch_add(1, Ordering::SeqCst);
        if attempt < 2 {
            Ok(Box::pin(stream::iter([Err(rate_limit_error())])))
        } else {
            Ok(Box::pin(stream::iter([Ok(
                ModelEvent::ResponseCompleted {
                    response: Box::new(response()),
                },
            )])))
        }
    }
}

#[tokio::test]
async fn retrying_port_retries_transient_generate_failures() {
    let inner = Box::new(FlakyProvider {
        attempts: AtomicUsize::new(0),
    });
    let policy = RetryPolicy::new(3, Duration::ZERO, Duration::ZERO).with_jitter(false);
    let provider = RetryingModelPort::new(inner, policy);

    let mut result = provider
        .invoke(request(), ModelCallOptions::new("attempt"))
        .await
        .unwrap();
    let completed = result.next().await.unwrap().unwrap();
    assert!(
        matches!(completed, ModelEvent::ResponseCompleted { response } if response.text == "ok")
    );
}

#[derive(Default)]
struct TransportSignalCollector {
    signals: Mutex<Vec<ModelTransportSignal>>,
}

impl ModelTransportObserver for TransportSignalCollector {
    fn observe(&self, signal: ModelTransportSignal) {
        self.signals.lock().unwrap().push(signal);
    }
}

#[tokio::test]
async fn retrying_port_reports_each_real_transport_attempt() {
    let collector = Arc::new(TransportSignalCollector::default());
    let inner = Box::new(FlakyProvider {
        attempts: AtomicUsize::new(0),
    });
    let policy = RetryPolicy::new(3, Duration::ZERO, Duration::ZERO).with_jitter(false);
    let provider = RetryingModelPort::new(inner, policy);
    let options =
        ModelCallOptions::new("model-attempt-1").with_transport_observer(collector.clone());

    let mut stream = provider.invoke(request(), options).await.unwrap();
    let _ = stream.next().await.unwrap().unwrap();

    let signals = collector.signals.lock().unwrap();
    assert_eq!(signals.len(), 6);
    assert!(matches!(signals[0].kind, ModelTransportSignalKind::Started));
    assert!(matches!(
        signals[1].kind,
        ModelTransportSignalKind::Failed {
            retry_delay_ms: Some(0),
            ..
        }
    ));
    assert!(matches!(signals[2].kind, ModelTransportSignalKind::Started));
    assert!(matches!(
        signals[3].kind,
        ModelTransportSignalKind::Failed {
            retry_delay_ms: Some(0),
            ..
        }
    ));
    assert!(matches!(signals[4].kind, ModelTransportSignalKind::Started));
    assert!(matches!(
        signals[5].kind,
        ModelTransportSignalKind::Succeeded { .. }
    ));
    assert!(
        signals
            .iter()
            .all(|signal| signal.model_attempt_id == "model-attempt-1")
    );
    assert_eq!(signals[5].transport_attempt, 3);
}

struct InterruptedStreamProvider {
    attempts: Arc<AtomicUsize>,
    invocations: Arc<Mutex<usize>>,
}

#[async_trait]
impl ModelPort for InterruptedStreamProvider {
    async fn invoke(
        &self,
        _request: ModelRequest,
        _options: ModelCallOptions,
    ) -> Result<ModelStream, ModelError> {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        *self.invocations.lock().unwrap() += 1;
        Ok(Box::pin(stream::iter([
            Ok(ModelEvent::TextDelta {
                index: 0,
                delta: "partial".to_string(),
            }),
            Err(ModelError::network("stream disconnected")),
        ])))
    }
}

#[tokio::test]
async fn retrying_port_never_retries_after_semantic_stream_output() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let invocations = Arc::new(Mutex::new(0));
    let inner = Box::new(InterruptedStreamProvider {
        attempts: Arc::clone(&attempts),
        invocations: Arc::clone(&invocations),
    });
    let policy = RetryPolicy::new(3, Duration::ZERO, Duration::ZERO).with_jitter(false);
    let provider = RetryingModelPort::new(inner, policy);

    let mut stream = provider
        .invoke(request(), ModelCallOptions::new("attempt"))
        .await
        .unwrap();
    assert!(matches!(
        stream.next().await,
        Some(Ok(ModelEvent::TextDelta { .. }))
    ));
    let error = stream.next().await.unwrap().unwrap_err();

    assert_eq!(error.code(), ModelErrorCode::Network);
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    assert_eq!(*invocations.lock().unwrap(), 1);
}

struct DropSignalStream {
    dropped: Arc<AtomicBool>,
}

impl Stream for DropSignalStream {
    type Item = Result<ModelEvent, ModelError>;

    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Pending
    }
}

impl Drop for DropSignalStream {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

struct PendingProvider {
    dropped: Arc<AtomicBool>,
    invoked: Arc<AtomicBool>,
}

#[async_trait]
impl ModelPort for PendingProvider {
    async fn invoke(
        &self,
        _request: ModelRequest,
        _options: ModelCallOptions,
    ) -> Result<ModelStream, ModelError> {
        self.invoked.store(true, Ordering::SeqCst);
        Ok(Box::pin(DropSignalStream {
            dropped: Arc::clone(&self.dropped),
        }))
    }
}

#[tokio::test]
async fn dropping_retry_stream_cancels_in_flight_attempt() {
    let dropped = Arc::new(AtomicBool::new(false));
    let invoked = Arc::new(AtomicBool::new(false));
    let provider = RetryingModelPort::new(
        Box::new(PendingProvider {
            dropped: Arc::clone(&dropped),
            invoked: Arc::clone(&invoked),
        }),
        RetryPolicy::default(),
    );
    let mut stream = provider
        .invoke(request(), ModelCallOptions::new("attempt"))
        .await
        .unwrap();
    assert!(!invoked.load(Ordering::SeqCst));

    let polling = tokio::spawn(async move { stream.next().await });
    while !invoked.load(Ordering::SeqCst) {
        tokio::task::yield_now().await;
    }
    polling.abort();
    let _ = polling.await;

    for _ in 0..20 {
        if dropped.load(Ordering::SeqCst) {
            return;
        }
        tokio::task::yield_now().await;
    }
    assert!(dropped.load(Ordering::SeqCst));
}
