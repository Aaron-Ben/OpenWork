use std::time::Duration;

use openwork_protocol::model::ModelErrorCode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelTransportSignal {
    AttemptStarted {
        attempt_no: usize,
    },
    RetryScheduled {
        attempt_no: usize,
        delay: Duration,
    },
    AttemptFailed {
        attempt_no: usize,
        error_code: ModelErrorCode,
    },
}
