use std::time::Duration;

use openwork_core::{SessionUpdate, SessionUpdateEnvelope};
use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast;
use tokio::time::Instant;

pub const SESSION_UPDATE_EVENT: &str = "openwork://session-update";
pub const SESSION_UPDATE_BATCH_EVENT: &str = "openwork://session-update-batch";
const LIVE_UPDATE_BATCH_INTERVAL: Duration = Duration::from_millis(50);

enum BridgeEmission {
    Single(SessionUpdateEnvelope),
    LiveBatch(Vec<SessionUpdateEnvelope>),
}

#[derive(Default)]
struct UpdateBatcher {
    pending_live: Vec<SessionUpdateEnvelope>,
}

impl UpdateBatcher {
    fn push(&mut self, payload: SessionUpdateEnvelope) -> Vec<BridgeEmission> {
        if matches!(
            &payload.update,
            SessionUpdate::TextDelta { .. }
                | SessionUpdate::ReasoningDelta { .. }
                | SessionUpdate::ToolCallProgress { .. }
        ) {
            self.pending_live.push(payload);
            return Vec::new();
        }

        let mut emissions = Vec::with_capacity(2);
        if let Some(batch) = self.flush() {
            emissions.push(batch);
        }
        emissions.push(BridgeEmission::Single(payload));
        emissions
    }

    fn flush(&mut self) -> Option<BridgeEmission> {
        if self.pending_live.is_empty() {
            return None;
        }
        Some(BridgeEmission::LiveBatch(std::mem::take(
            &mut self.pending_live,
        )))
    }

    fn has_pending(&self) -> bool {
        !self.pending_live.is_empty()
    }
}

fn emit(app: &AppHandle, emission: BridgeEmission) {
    match emission {
        BridgeEmission::Single(payload) => {
            let _ = app.emit(SESSION_UPDATE_EVENT, payload);
        }
        BridgeEmission::LiveBatch(payloads) => {
            let _ = app.emit(SESSION_UPDATE_BATCH_EVENT, payloads);
        }
    }
}

pub fn spawn_session_update_bridge(
    app: AppHandle,
    mut updates: broadcast::Receiver<SessionUpdateEnvelope>,
) {
    tauri::async_runtime::spawn(async move {
        let mut batcher = UpdateBatcher::default();
        let flush_timer = tokio::time::sleep(LIVE_UPDATE_BATCH_INTERVAL);
        tokio::pin!(flush_timer);

        loop {
            tokio::select! {
                result = updates.recv() => match result {
                    Ok(payload) => {
                        let was_empty = !batcher.has_pending();
                        for emission in batcher.push(payload) {
                            emit(&app, emission);
                        }
                        if was_empty && batcher.has_pending() {
                            flush_timer
                                .as_mut()
                                .reset(Instant::now() + LIVE_UPDATE_BATCH_INTERVAL);
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        if let Some(emission) = batcher.flush() {
                            emit(&app, emission);
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        if let Some(emission) = batcher.flush() {
                            emit(&app, emission);
                        }
                        break;
                    }
                },
                _ = &mut flush_timer, if batcher.has_pending() => {
                    if let Some(emission) = batcher.flush() {
                        emit(&app, emission);
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use openwork_core::SessionUpdateEnvelope;
    use serde_json::json;

    use super::{BridgeEmission, UpdateBatcher};

    fn envelope(sequence: u64, update: serde_json::Value) -> SessionUpdateEnvelope {
        serde_json::from_value(json!({
            "version": 6,
            "sessionId": "session-1",
            "turnId": "turn-1",
            "sequence": sequence,
            "occurredAtMs": sequence,
            "update": update,
        }))
        .expect("valid session update envelope")
    }

    fn progress(sequence: u64) -> SessionUpdateEnvelope {
        envelope(
            sequence,
            json!({
                "type": "tool_call_progress",
                "toolCallId": "tool-1",
                "progress": { "kind": "stdout", "chunk": format!("line {sequence}\n") },
            }),
        )
    }

    fn reasoning(sequence: u64) -> SessionUpdateEnvelope {
        envelope(
            sequence,
            json!({
                "type": "reasoning_delta",
                "delta": format!("thought {sequence} "),
            }),
        )
    }

    fn text(sequence: u64) -> SessionUpdateEnvelope {
        envelope(
            sequence,
            json!({
                "type": "text_delta",
                "delta": format!("answer {sequence} "),
            }),
        )
    }

    #[test]
    fn batches_progress_and_flushes_it_before_the_next_state_update() {
        let mut batcher = UpdateBatcher::default();

        assert!(batcher.push(progress(1)).is_empty());
        assert!(batcher.push(progress(2)).is_empty());
        let emissions = batcher.push(envelope(
            3,
            json!({ "type": "phase_changed", "phase": "running_model" }),
        ));

        assert_eq!(emissions.len(), 2);
        assert!(matches!(
            &emissions[0],
            BridgeEmission::LiveBatch(batch)
                if batch.iter().map(|item| item.sequence).collect::<Vec<_>>() == vec![1, 2]
        ));
        assert!(matches!(
            &emissions[1],
            BridgeEmission::Single(item) if item.sequence == 3
        ));
    }

    #[test]
    fn timer_flush_preserves_every_live_sequence() {
        let mut batcher = UpdateBatcher::default();
        for sequence in 1..=200 {
            let payload = match sequence % 3 {
                0 => progress(sequence),
                1 => reasoning(sequence),
                _ => text(sequence),
            };
            assert!(batcher.push(payload).is_empty());
        }

        let BridgeEmission::LiveBatch(batch) = batcher
            .flush()
            .expect("pending progress should flush as one batch")
        else {
            panic!("expected a progress batch")
        };
        assert_eq!(batch.len(), 200);
        assert_eq!(batch.first().map(|item| item.sequence), Some(1));
        assert_eq!(batch.last().map(|item| item.sequence), Some(200));
    }
}
