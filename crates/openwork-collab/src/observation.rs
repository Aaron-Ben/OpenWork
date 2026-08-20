//! Best-effort durable observations and OpenCode event normalization.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, RwLock},
};

use serde_json::{Value, json};
use tokio::{sync::mpsc, task::JoinHandle};
use tokio_util::sync::CancellationToken;

use crate::{
    event::{CollabEventKind, CollabEventPublisher},
    model::{ObservationInput, RunOutcome, TriageRecord, TriageRecordInput},
    opencode::GlobalEvent,
    storage::{CollabStorage, StorageError},
};

const MAX_DETAIL_CHARS: usize = 2_000;
const MAX_TRACKED_RUNS: usize = 1_024;
const MAX_FINGERPRINTS_PER_RUN: usize = 512;
const MAX_SILENT_ASSISTANT_CHARS: usize = 8;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunEvidence {
    acted_rooms: HashSet<String>,
    acked_rooms: HashSet<String>,
    assistant_message_ids: HashSet<String>,
    text_parts: HashMap<String, usize>,
}

impl RunEvidence {
    pub fn outcome(&self) -> RunOutcome {
        if !self.acted_rooms.is_empty() {
            RunOutcome::Acted
        } else if self.text_parts.values().sum::<usize>() > MAX_SILENT_ASSISTANT_CHARS {
            RunOutcome::Unpublished
        } else {
            RunOutcome::Silent
        }
    }

    /// Rooms whose delivered batch this run finished with: either it published
    /// something there, or it explicitly stood down with `ack`.
    ///
    /// This is the only thing that may advance a read cursor. A room the engine
    /// was shown but never closed out stays unread on purpose — the whole point
    /// of per-room settlement is that a turn focused elsewhere cannot silently
    /// mark it handled.
    pub fn settled_rooms(&self) -> impl Iterator<Item = &str> {
        self.acted_rooms
            .iter()
            .chain(self.acked_rooms.iter())
            .map(String::as_str)
    }

    pub fn acted_rooms(&self) -> &HashSet<String> {
        &self.acted_rooms
    }

    pub fn acked_rooms(&self) -> &HashSet<String> {
        &self.acked_rooms
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenUsage {
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub output_tokens: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EngineObservation {
    pub kind: &'static str,
    pub payload: Value,
    pub usage: Option<TokenUsage>,
}

pub fn normalize_engine_observation(event: &GlobalEvent) -> Option<EngineObservation> {
    match event.event_type()? {
        "message.updated" => normalize_usage(event),
        "message.part.updated" => normalize_tool(event),
        "session.next.shell.started" | "session.next.shell.ended" => normalize_shell(event),
        _ => None,
    }
}

pub fn record_engine_observation(
    observations: &ObservationSink,
    run_id: &str,
    agent_id: &str,
    room_id: Option<&str>,
    event: &GlobalEvent,
) {
    let Some(normalized) = normalize_engine_observation(event) else {
        return;
    };
    observations.record(
        OwnedObservation::linked(
            Some(run_id),
            Some(agent_id),
            room_id,
            normalized.kind,
            normalized.payload,
        )
        .with_usage(normalized.usage),
    );
}

fn normalize_usage(event: &GlobalEvent) -> Option<EngineObservation> {
    let info = event.payload.pointer("/properties/info")?;
    if info.get("role").and_then(Value::as_str) != Some("assistant") {
        return None;
    }
    let tokens = info.get("tokens")?;
    let usage = TokenUsage {
        input_tokens: tokens.get("input")?.as_i64()?,
        cached_input_tokens: tokens.pointer("/cache/read")?.as_i64()?,
        output_tokens: tokens.get("output")?.as_i64()?,
    };
    let reasoning_tokens = tokens.get("reasoning").and_then(Value::as_i64);
    let cache_write_tokens = tokens.pointer("/cache/write").and_then(Value::as_i64);
    if usage == TokenUsage::default()
        && reasoning_tokens.unwrap_or(0) == 0
        && cache_write_tokens.unwrap_or(0) == 0
    {
        return None;
    }
    Some(EngineObservation {
        kind: "usage.reported",
        payload: json!({
            "messageId": info.get("id").and_then(Value::as_str),
            "inputTokens": usage.input_tokens,
            "cachedInputTokens": usage.cached_input_tokens,
            "outputTokens": usage.output_tokens,
            "reasoningTokens": reasoning_tokens,
            "cacheWriteTokens": cache_write_tokens,
        }),
        usage: Some(usage),
    })
}

fn normalize_tool(event: &GlobalEvent) -> Option<EngineObservation> {
    let part = event.payload.pointer("/properties/part")?;
    if part.get("type").and_then(Value::as_str) != Some("tool") {
        return None;
    }
    let tool = part.get("tool").and_then(Value::as_str)?;
    let status = part.pointer("/state/status").and_then(Value::as_str)?;
    let command = (tool == "bash")
        .then(|| part.pointer("/state/input/command").and_then(Value::as_str))
        .flatten()
        .map(bounded);
    Some(EngineObservation {
        kind: if tool == "bash" {
            "command.execution"
        } else {
            "tool.execution"
        },
        payload: json!({
            "partId": part.get("id").and_then(Value::as_str),
            "callId": part.get("callID").and_then(Value::as_str),
            "tool": tool,
            "status": status,
            "title": part.pointer("/state/title").and_then(Value::as_str).map(bounded),
            "command": command,
            "error": part.pointer("/state/error").and_then(Value::as_str).map(bounded),
        }),
        usage: None,
    })
}

fn normalize_shell(event: &GlobalEvent) -> Option<EngineObservation> {
    let properties = event.payload.get("properties")?;
    Some(EngineObservation {
        kind: "command.execution",
        payload: json!({
            "callId": properties.get("callID").and_then(Value::as_str),
            "command": properties.get("command").and_then(Value::as_str).map(bounded),
            "status": if event.event_type()? == "session.next.shell.started" { "running" } else { "completed" },
        }),
        usage: None,
    })
}

fn bounded(value: &str) -> String {
    value.chars().take(MAX_DETAIL_CHARS).collect()
}

#[derive(Debug, Clone)]
pub struct OwnedObservation {
    run_id: Option<String>,
    active_run_agent_id: Option<String>,
    agent_id: Option<String>,
    room_id: Option<String>,
    kind: String,
    payload: Value,
    usage: Option<TokenUsage>,
}

impl OwnedObservation {
    pub fn linked(
        run_id: Option<&str>,
        agent_id: Option<&str>,
        room_id: Option<&str>,
        kind: impl Into<String>,
        payload: Value,
    ) -> Self {
        Self {
            run_id: run_id.map(str::to_string),
            active_run_agent_id: None,
            agent_id: agent_id.map(str::to_string),
            room_id: room_id.map(str::to_string),
            kind: kind.into(),
            payload,
            usage: None,
        }
    }

    pub fn for_active_run(
        agent_id: &str,
        room_id: Option<&str>,
        kind: impl Into<String>,
        payload: Value,
    ) -> Self {
        Self {
            run_id: None,
            active_run_agent_id: Some(agent_id.to_string()),
            agent_id: Some(agent_id.to_string()),
            room_id: room_id.map(str::to_string),
            kind: kind.into(),
            payload,
            usage: None,
        }
    }

    pub fn with_usage(mut self, usage: Option<TokenUsage>) -> Self {
        self.usage = usage;
        self
    }
}

#[derive(Clone)]
pub struct ObservationSink {
    sender: Option<mpsc::Sender<OwnedObservation>>,
    active_runs: Arc<RwLock<HashMap<String, String>>>,
    run_evidence: Arc<RwLock<HashMap<String, RunEvidence>>>,
}

impl ObservationSink {
    pub fn record(&self, mut event: OwnedObservation) {
        if event.run_id.is_none()
            && let Some(agent_id) = event.active_run_agent_id.as_deref()
            && let Ok(active_runs) = self.active_runs.try_read()
        {
            event.run_id = active_runs.get(agent_id).cloned();
        }
        if let Some(sender) = &self.sender
            && let Err(error) = sender.try_send(event)
        {
            eprintln!("collaboration observation dropped: {error}");
        }
    }

    pub fn discarding() -> Self {
        Self {
            sender: None,
            active_runs: Arc::default(),
            run_evidence: Arc::default(),
        }
    }

    pub fn set_active_run(&self, agent_id: &str, run_id: Option<&str>) {
        let Ok(mut active_runs) = self.active_runs.write() else {
            eprintln!("collaboration active-run observation lock is poisoned");
            return;
        };
        match run_id {
            Some(run_id) => {
                active_runs.insert(agent_id.to_string(), run_id.to_string());
                if let Ok(mut evidence) = self.run_evidence.write() {
                    evidence.entry(run_id.to_string()).or_default();
                }
            }
            None => {
                active_runs.remove(agent_id);
            }
        }
    }

    /// Record that the Agent published something into `room_id` this run.
    pub fn mark_action(&self, agent_id: &str, room_id: &str) {
        self.with_active_evidence(agent_id, |evidence| {
            evidence.acted_rooms.insert(room_id.to_string());
        });
    }

    /// Record that the Agent explicitly stood down on `room_id` this run.
    ///
    /// Standing down is not acting: it settles the room's delivery without
    /// counting as a response, so an ack-only turn still reports `silent`.
    pub fn mark_ack(&self, agent_id: &str, room_id: &str) {
        self.with_active_evidence(agent_id, |evidence| {
            evidence.acked_rooms.insert(room_id.to_string());
        });
    }

    fn with_active_evidence(&self, agent_id: &str, apply: impl FnOnce(&mut RunEvidence)) {
        let Some(run_id) = self
            .active_runs
            .read()
            .ok()
            .and_then(|runs| runs.get(agent_id).cloned())
        else {
            return;
        };
        if let Ok(mut evidence) = self.run_evidence.write() {
            apply(evidence.entry(run_id).or_default());
        }
    }

    pub fn observe_assistant_text(&self, run_id: &str, event: &GlobalEvent) {
        if event.event_type() == Some("message.updated") {
            let Some(info) = event.payload.pointer("/properties/info") else {
                return;
            };
            if info.get("role").and_then(Value::as_str) != Some("assistant") {
                return;
            }
            let Some(message_id) = info.get("id").and_then(Value::as_str) else {
                return;
            };
            if let Ok(mut evidence) = self.run_evidence.write() {
                evidence
                    .entry(run_id.to_string())
                    .or_default()
                    .assistant_message_ids
                    .insert(message_id.to_string());
            }
            return;
        }
        if event.event_type() != Some("message.part.updated") {
            return;
        }
        let Some(part) = event.payload.pointer("/properties/part") else {
            return;
        };
        if part.get("type").and_then(Value::as_str) != Some("text") {
            return;
        }
        let Some(text) = part.get("text").and_then(Value::as_str) else {
            return;
        };
        let Some(message_id) = part
            .get("messageID")
            .or_else(|| part.get("messageId"))
            .and_then(Value::as_str)
        else {
            return;
        };
        let part_id = part
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let chars = text
            .chars()
            .filter(|character| !character.is_whitespace())
            .count();
        if let Ok(mut evidence) = self.run_evidence.write() {
            let evidence = evidence.entry(run_id.to_string()).or_default();
            if !evidence.assistant_message_ids.contains(message_id) {
                return;
            }
            evidence
                .text_parts
                .entry(part_id)
                .and_modify(|current| *current = (*current).max(chars))
                .or_insert(chars);
        }
    }

    pub fn take_run_evidence(&self, run_id: &str) -> RunEvidence {
        self.run_evidence
            .write()
            .ok()
            .and_then(|mut evidence| evidence.remove(run_id))
            .unwrap_or_default()
    }
}

pub struct ObservationHandle(JoinHandle<()>);

impl ObservationHandle {
    pub async fn shutdown(self) {
        let _ = self.0.await;
    }
}

pub fn start(
    storage: CollabStorage,
    published_events: CollabEventPublisher,
    cancel: CancellationToken,
) -> (ObservationSink, ObservationHandle) {
    let (sender, mut receiver) = mpsc::channel::<OwnedObservation>(1_024);
    let active_runs = Arc::new(RwLock::new(HashMap::new()));
    let run_evidence = Arc::new(RwLock::new(HashMap::new()));
    let task = tokio::spawn(async move {
        let mut run_usage = HashMap::<String, HashMap<String, TokenUsage>>::new();
        let mut run_fingerprints = HashMap::<String, HashSet<String>>::new();
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return,
                event = receiver.recv() => {
                    let Some(event) = event else { return };
                    let run_id = event.run_id.clone();
                    let terminal_prompt = matches!(
                        event.kind.as_str(),
                        "prompt.completed" | "prompt.failed" | "prompt.interrupted" | "prompt.cancelled"
                    );
                    let message_id = event
                        .payload
                        .get("messageId")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    if let Some(run_id) = run_id.as_deref() {
                        if !run_fingerprints.contains_key(run_id)
                            && run_fingerprints.len() >= MAX_TRACKED_RUNS
                        {
                            run_fingerprints.clear();
                        }
                        let fingerprints = run_fingerprints
                            .entry(run_id.to_string())
                            .or_default();
                        if fingerprints.len() >= MAX_FINGERPRINTS_PER_RUN {
                            fingerprints.clear();
                        }
                        let fingerprint = format!("{}:{}", event.kind, event.payload);
                        if !fingerprints.insert(fingerprint) {
                            continue;
                        }
                    }
                    let observation_inserted = match storage.insert_observation(ObservationInput {
                        run_id: event.run_id.as_deref(),
                        agent_id: event.agent_id.as_deref(),
                        room_id: event.room_id.as_deref(),
                        kind: &event.kind,
                        payload: event.payload,
                    }).await {
                        Ok(_) => true,
                        Err(error) => {
                            eprintln!("failed to write collaboration observation: {error}");
                            false
                        }
                    };
                    if let (Some(run_id), Some(message_id), Some(usage)) =
                        (run_id, message_id, event.usage)
                    {
                        if !run_usage.contains_key(&run_id) && run_usage.len() >= MAX_TRACKED_RUNS {
                            run_usage.clear();
                        }
                        let messages = run_usage.entry(run_id.clone()).or_default();
                        messages.insert(message_id, usage);
                        let total = messages.values().fold(TokenUsage::default(), |total, usage| {
                            TokenUsage {
                                input_tokens: total.input_tokens.saturating_add(usage.input_tokens),
                                cached_input_tokens: total.cached_input_tokens.saturating_add(usage.cached_input_tokens),
                                output_tokens: total.output_tokens.saturating_add(usage.output_tokens),
                            }
                        });
                        if let Err(error) = storage.update_run_usage(&run_id, total).await {
                            eprintln!("failed to update collaboration run usage: {error}");
                        }
                    }
                    if observation_inserted {
                        published_events.publish(CollabEventKind::LogsChanged {
                            room_id: event.room_id.clone(),
                        }).await;
                    }
                    if terminal_prompt && let Some(run_id) = event.run_id.as_deref() {
                        run_usage.remove(run_id);
                        run_fingerprints.remove(run_id);
                    }
                }
            }
        }
    });
    (
        ObservationSink {
            sender: Some(sender),
            active_runs,
            run_evidence,
        },
        ObservationHandle(task),
    )
}

pub async fn record_triage(
    storage: &CollabStorage,
    observations: &ObservationSink,
    input: TriageRecordInput<'_>,
) -> Result<TriageRecord, StorageError> {
    let record = storage.record_triage(input).await?;
    observations.record(OwnedObservation::linked(
        None,
        Some(&record.agent_id),
        Some(&record.room_id),
        "triage.decision",
        serde_json::to_value(&record).unwrap_or_else(|error| {
            json!({
                "triageId": record.id,
                "serializationError": error.to_string(),
            })
        }),
    ));
    Ok(record)
}
