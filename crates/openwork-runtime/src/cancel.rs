use std::collections::HashMap;
use std::sync::Mutex;

use tokio_util::sync::CancellationToken;

/// Tracks cancellation tokens by request id.
#[derive(Default)]
pub struct RequestCancelRegistry {
    tokens: Mutex<HashMap<String, CancellationToken>>,
}

impl RequestCancelRegistry {
    /// Registers a request id and returns the token passed into the agent loop.
    pub fn register(&self, request_id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        self.tokens
            .lock()
            .expect("cancel registry mutex poisoned")
            .insert(request_id.to_string(), token.clone());
        token
    }

    /// Cancels and removes a request token. Returns whether a request was found.
    pub fn cancel(&self, request_id: &str) -> bool {
        if let Some(token) = self
            .tokens
            .lock()
            .expect("cancel registry mutex poisoned")
            .remove(request_id)
        {
            token.cancel();
            true
        } else {
            false
        }
    }

    /// Removes a completed or failed request token.
    pub fn remove(&self, request_id: &str) {
        self.tokens
            .lock()
            .expect("cancel registry mutex poisoned")
            .remove(request_id);
    }
}
