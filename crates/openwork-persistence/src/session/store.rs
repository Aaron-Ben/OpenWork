use std::{collections::HashMap, sync::Arc, time::SystemTime};

use openwork_protocol::{
    domain::EventId,
    journal::{
        AggregateType, EventJournal, EventJournalError, ExpectedVersion, NewRecordedEventV1,
        RecordedEventV1,
    },
    model::Role,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::types::{
    NewMessage, Session, SessionInput, SessionMessage, SessionSummary, TurnOutcome,
};

const EVENT_PAGE_SIZE: u32 = 1_000;
const THREAD_CREATED: &str = "thread_created";
const THREAD_TITLE_CHANGED: &str = "thread_title_changed";
const THREAD_DELETED: &str = "thread_deleted";
const TURN_STARTED: &str = "turn_started";
const TURN_COMPLETED: &str = "turn_completed";
const TURN_CANCELLED: &str = "turn_cancelled";
const TURN_DOOM_LOOP: &str = "turn_doom_loop_detected";
const TURN_FAILED: &str = "turn_failed";

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("session not found: {id}")]
    NotFound { id: String },
    #[error("turn not found: {id}")]
    TurnNotFound { id: String },
    #[error("turn is already terminal: {id}")]
    TurnAlreadyTerminal { id: String },
    #[error("invalid session event: {message}")]
    InvalidEvent { message: String },
    #[error("event journal error: {0}")]
    Journal(#[from] EventJournalError),
    #[error("session serialize error: {0}")]
    Serialize(#[from] serde_json::Error),
}

#[derive(Clone)]
pub struct SessionStore {
    journal: Arc<dyn EventJournal>,
}

impl SessionStore {
    pub fn new(journal: Arc<dyn EventJournal>) -> Self {
        Self { journal }
    }

    pub async fn list_sessions(&self) -> Result<Vec<SessionSummary>, SessionError> {
        let events = self.read_all_events().await?;
        let mut sessions = project_sessions(&events)?;
        sessions.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(sessions
            .into_iter()
            .map(|session| SessionSummary {
                id: session.id,
                title: session.title,
                provider_id: session.provider_id,
                model: session.model,
                working_dir: session.working_dir,
                updated_at: session.updated_at,
            })
            .collect())
    }

    pub async fn create_session(&self, input: SessionInput) -> Result<Session, SessionError> {
        let now_ms = now_unix_ms();
        let session = Session {
            id: generate_id("sess"),
            title: input
                .title
                .filter(|title| !title.trim().is_empty())
                .unwrap_or_else(|| "New session".to_string()),
            provider_id: input.provider_id,
            model: input.model,
            working_dir: input.working_dir,
            created_at: now_ms / 1_000,
            updated_at: now_ms / 1_000,
        };
        let payload = ThreadCreatedPayload {
            title: session.title.clone(),
            provider_id: session.provider_id.clone(),
            model: session.model.clone(),
            working_dir: session.working_dir.clone(),
        };
        self.journal
            .append(
                AggregateType::Thread,
                &session.id,
                ExpectedVersion::NoStream,
                vec![new_event(THREAD_CREATED, &payload, now_ms)?],
            )
            .await?;
        Ok(session)
    }

    pub async fn load_session(&self, id: &str) -> Result<Option<Session>, SessionError> {
        Ok(project_sessions(&self.read_all_events().await?)?
            .into_iter()
            .find(|session| session.id == id))
    }

    pub async fn load_messages(
        &self,
        session_id: &str,
    ) -> Result<Vec<SessionMessage>, SessionError> {
        if self.load_session(session_id).await?.is_none() {
            return Err(SessionError::NotFound {
                id: session_id.to_string(),
            });
        }
        let mut messages = self
            .read_all_events()
            .await?
            .into_iter()
            .filter(|event| {
                event.aggregate_type == AggregateType::Turn && is_message_event(&event.event_type)
            })
            .map(|event| {
                let payload: MessageRecordedPayload =
                    serde_json::from_value(event.payload.clone())?;
                Ok((event, payload))
            })
            .collect::<Result<Vec<_>, serde_json::Error>>()?
            .into_iter()
            .filter(|(_, payload)| payload.thread_id == session_id)
            .enumerate()
            .map(|(index, (event, payload))| SessionMessage {
                id: payload.message_id,
                session_id: payload.thread_id,
                role: payload.role,
                parts: payload.parts,
                seq: index as i64 + 1,
                created_at: event.occurred_at_unix_ms / 1_000,
            })
            .collect::<Vec<_>>();
        messages.sort_by_key(|message| message.seq);
        Ok(messages)
    }

    pub async fn rename_session(&self, id: &str, title: &str) -> Result<Session, SessionError> {
        let Some(_) = self.load_session(id).await? else {
            return Err(SessionError::NotFound { id: id.to_string() });
        };
        if title.trim().is_empty() {
            return Err(SessionError::InvalidEvent {
                message: "session title must not be blank".to_string(),
            });
        }
        self.append_thread_event(
            id,
            THREAD_TITLE_CHANGED,
            &ThreadTitleChangedPayload {
                title: title.to_string(),
            },
        )
        .await?;
        self.load_session(id)
            .await?
            .ok_or_else(|| SessionError::NotFound { id: id.to_string() })
    }

    pub async fn delete_session(&self, id: &str) -> Result<(), SessionError> {
        let Some(_) = self.load_session(id).await? else {
            return Err(SessionError::NotFound { id: id.to_string() });
        };
        self.append_thread_event(id, THREAD_DELETED, &EmptyPayload {})
            .await
            .map(|_| ())
    }

    pub async fn start_turn(
        &self,
        turn_id: &str,
        session_id: &str,
        user_message: NewMessage,
    ) -> Result<(), SessionError> {
        let session =
            self.load_session(session_id)
                .await?
                .ok_or_else(|| SessionError::NotFound {
                    id: session_id.to_string(),
                })?;
        if user_message.role != Role::User {
            return Err(SessionError::InvalidEvent {
                message: "start_turn requires a user message".to_string(),
            });
        }
        let now_ms = now_unix_ms();
        let turn = TurnStartedPayload {
            thread_id: session_id.to_string(),
            provider_id: session.provider_id,
            model: session.model,
        };
        let message = message_payload(session_id, user_message);
        self.journal
            .append(
                AggregateType::Turn,
                turn_id,
                ExpectedVersion::NoStream,
                vec![
                    new_event(TURN_STARTED, &turn, now_ms)?,
                    new_event("user_message_recorded", &message, now_ms)?,
                ],
            )
            .await?;
        Ok(())
    }

    pub async fn finish_turn(
        &self,
        turn_id: &str,
        session_id: &str,
        messages: Vec<NewMessage>,
        outcome: TurnOutcome,
    ) -> Result<(), SessionError> {
        if self.load_session(session_id).await?.is_none() {
            return Err(SessionError::NotFound {
                id: session_id.to_string(),
            });
        }
        let existing = self
            .journal
            .load_aggregate(AggregateType::Turn, turn_id, 0)
            .await?;
        let Some(last) = existing.last() else {
            return Err(SessionError::TurnNotFound {
                id: turn_id.to_string(),
            });
        };
        let started = existing
            .iter()
            .find(|event| event.event_type == TURN_STARTED)
            .ok_or_else(|| SessionError::InvalidEvent {
                message: format!("turn {turn_id} has no turn_started event"),
            })?;
        let started: TurnStartedPayload = serde_json::from_value(started.payload.clone())?;
        if started.thread_id != session_id {
            return Err(SessionError::InvalidEvent {
                message: format!(
                    "turn {turn_id} belongs to thread {}, not {session_id}",
                    started.thread_id
                ),
            });
        }
        if existing.iter().any(|event| is_terminal(&event.event_type)) {
            return Err(SessionError::TurnAlreadyTerminal {
                id: turn_id.to_string(),
            });
        }

        let now_ms = now_unix_ms();
        let mut events = messages
            .into_iter()
            .filter(|message| message.role != Role::System)
            .map(|message| {
                let event_type = message_event_type(message.role);
                new_event(event_type, &message_payload(session_id, message), now_ms)
            })
            .collect::<Result<Vec<_>, SessionError>>()?;
        let (terminal_type, terminal_payload) = terminal_event(outcome)?;
        events.push(NewRecordedEventV1::new(
            EventId::new(generate_id("evt")),
            terminal_type,
            terminal_payload,
            now_ms,
        ));
        self.journal
            .append(
                AggregateType::Turn,
                turn_id,
                ExpectedVersion::Exact(last.aggregate_version),
                events,
            )
            .await?;
        Ok(())
    }

    async fn append_thread_event(
        &self,
        id: &str,
        event_type: &str,
        payload: &impl Serialize,
    ) -> Result<Vec<RecordedEventV1>, SessionError> {
        let existing = self
            .journal
            .load_aggregate(AggregateType::Thread, id, 0)
            .await?;
        let Some(last) = existing.last() else {
            return Err(SessionError::NotFound { id: id.to_string() });
        };
        let now_ms = now_unix_ms();
        Ok(self
            .journal
            .append(
                AggregateType::Thread,
                id,
                ExpectedVersion::Exact(last.aggregate_version),
                vec![new_event(event_type, payload, now_ms)?],
            )
            .await?)
    }

    async fn read_all_events(&self) -> Result<Vec<RecordedEventV1>, SessionError> {
        let mut position = 0;
        let mut events = Vec::new();
        loop {
            let page = self.journal.read_all(position, EVENT_PAGE_SIZE).await?;
            if page.is_empty() {
                break;
            }
            position = page.last().map_or(position, |event| event.global_position);
            let complete = page.len() < EVENT_PAGE_SIZE as usize;
            events.extend(page);
            if complete {
                break;
            }
        }
        Ok(events)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadCreatedPayload {
    title: String,
    provider_id: String,
    model: String,
    working_dir: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ThreadTitleChangedPayload {
    title: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TurnStartedPayload {
    thread_id: String,
    provider_id: String,
    model: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessageRecordedPayload {
    message_id: String,
    thread_id: String,
    role: Role,
    parts: Vec<openwork_protocol::model::ContentBlock>,
}

#[derive(Debug, Serialize)]
struct EmptyPayload {}

#[derive(Debug, Serialize)]
struct DoomLoopPayload {
    repeated: String,
}

#[derive(Debug, Serialize)]
struct FailedPayload {
    message: String,
}

fn project_sessions(events: &[RecordedEventV1]) -> Result<Vec<Session>, SessionError> {
    let mut states = HashMap::<String, (Session, bool)>::new();
    for event in events {
        if event.aggregate_type == AggregateType::Thread {
            match event.event_type.as_str() {
                THREAD_CREATED => {
                    let payload: ThreadCreatedPayload =
                        serde_json::from_value(event.payload.clone())?;
                    states.insert(
                        event.aggregate_id.clone(),
                        (
                            Session {
                                id: event.aggregate_id.clone(),
                                title: payload.title,
                                provider_id: payload.provider_id,
                                model: payload.model,
                                working_dir: payload.working_dir,
                                created_at: event.occurred_at_unix_ms / 1_000,
                                updated_at: event.occurred_at_unix_ms / 1_000,
                            },
                            false,
                        ),
                    );
                }
                THREAD_TITLE_CHANGED => {
                    if let Some((session, _)) = states.get_mut(&event.aggregate_id) {
                        let payload: ThreadTitleChangedPayload =
                            serde_json::from_value(event.payload.clone())?;
                        session.title = payload.title;
                        session.updated_at = event.occurred_at_unix_ms / 1_000;
                    }
                }
                THREAD_DELETED => {
                    if let Some((session, deleted)) = states.get_mut(&event.aggregate_id) {
                        *deleted = true;
                        session.updated_at = event.occurred_at_unix_ms / 1_000;
                    }
                }
                _ => {}
            }
        }
        if let Some(thread_id) = event
            .payload
            .get("threadId")
            .and_then(|value| value.as_str())
            && let Some((session, _)) = states.get_mut(thread_id)
        {
            session.updated_at = session.updated_at.max(event.occurred_at_unix_ms / 1_000);
        }
    }
    Ok(states
        .into_values()
        .filter_map(|(session, deleted)| (!deleted).then_some(session))
        .collect())
}

fn new_event(
    event_type: &str,
    payload: &impl Serialize,
    occurred_at_unix_ms: i64,
) -> Result<NewRecordedEventV1, SessionError> {
    Ok(NewRecordedEventV1::new(
        EventId::new(generate_id("evt")),
        event_type,
        serde_json::to_value(payload)?,
        occurred_at_unix_ms,
    ))
}

fn message_payload(thread_id: &str, message: NewMessage) -> MessageRecordedPayload {
    MessageRecordedPayload {
        message_id: generate_id("msg"),
        thread_id: thread_id.to_string(),
        role: message.role,
        parts: message.parts,
    }
}

fn message_event_type(role: Role) -> &'static str {
    match role {
        Role::System => "system_message_recorded",
        Role::User => "user_message_recorded",
        Role::Assistant => "assistant_message_recorded",
        Role::Tool => "tool_message_recorded",
    }
}

fn is_message_event(event_type: &str) -> bool {
    matches!(
        event_type,
        "system_message_recorded"
            | "user_message_recorded"
            | "assistant_message_recorded"
            | "tool_message_recorded"
    )
}

fn is_terminal(event_type: &str) -> bool {
    matches!(
        event_type,
        TURN_COMPLETED | TURN_CANCELLED | TURN_DOOM_LOOP | TURN_FAILED
    )
}

fn terminal_event(outcome: TurnOutcome) -> Result<(&'static str, serde_json::Value), SessionError> {
    match outcome {
        TurnOutcome::Completed => Ok((TURN_COMPLETED, serde_json::to_value(EmptyPayload {})?)),
        TurnOutcome::Cancelled => Ok((TURN_CANCELLED, serde_json::to_value(EmptyPayload {})?)),
        TurnOutcome::DoomLoop { repeated } => Ok((
            TURN_DOOM_LOOP,
            serde_json::to_value(DoomLoopPayload { repeated })?,
        )),
        TurnOutcome::Failed { message } => Ok((
            TURN_FAILED,
            serde_json::to_value(FailedPayload { message })?,
        )),
    }
}

fn generate_id(prefix: &str) -> String {
    format!("{prefix}-{}", Uuid::new_v4().simple())
}

fn now_unix_ms() -> i64 {
    let millis = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    i64::try_from(millis).unwrap_or(i64::MAX)
}
