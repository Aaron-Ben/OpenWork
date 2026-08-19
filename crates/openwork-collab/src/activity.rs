use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::sync::Mutex;

use crate::{model::AgentActivity, opencode::GlobalEvent};

const MAX_ACTIVITY_CHARS: usize = 80;

pub fn normalize_activity(event: &GlobalEvent) -> Option<AgentActivity> {
    match event.event_type()? {
        "session.status" => match event.session_status()? {
            "idle" => Some(AgentActivity::Idle),
            "busy" | "retry" => Some(AgentActivity::Busy),
            _ => None,
        },
        "session.idle" => Some(AgentActivity::Idle),
        "session.compacted" => Some(AgentActivity::Compacting),
        "message.updated"
            if event
                .payload
                .pointer("/properties/info/role")
                .and_then(serde_json::Value::as_str)
                == Some("assistant") =>
        {
            Some(AgentActivity::Replying)
        }
        "message.part.updated" => normalize_part(event),
        _ => None,
    }
}

fn normalize_part(event: &GlobalEvent) -> Option<AgentActivity> {
    let part = event.payload.pointer("/properties/part")?;
    match part.get("type").and_then(serde_json::Value::as_str)? {
        "compaction" => Some(AgentActivity::Compacting),
        "text" => Some(AgentActivity::Replying),
        "tool"
            if part
                .pointer("/state/status")
                .and_then(serde_json::Value::as_str)
                == Some("running") =>
        {
            let detail = part
                .pointer("/state/title")
                .or_else(|| part.get("tool"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("tool");
            Some(AgentActivity::Executing {
                detail: detail.chars().take(MAX_ACTIVITY_CHARS).collect(),
            })
        }
        _ => None,
    }
}

#[derive(Debug, Clone)]
struct RuntimeEntry {
    activity: AgentActivity,
    last_event: Instant,
}

#[derive(Debug, Default)]
struct RuntimeState {
    sessions: HashMap<String, String>,
    agents: HashMap<String, RuntimeEntry>,
}

#[derive(Debug, Clone, Default)]
pub struct AgentRuntimeRegistry {
    state: Arc<Mutex<RuntimeState>>,
}

impl AgentRuntimeRegistry {
    pub async fn register_session(&self, session_id: &str, agent_id: &str) {
        self.state
            .lock()
            .await
            .sessions
            .insert(session_id.to_string(), agent_id.to_string());
    }

    pub async fn observe(&self, event: &GlobalEvent) -> Option<(String, AgentActivity)> {
        let activity = normalize_activity(event)?;
        let session_id = event.session_id()?;
        let mut state = self.state.lock().await;
        let agent_id = state.sessions.get(session_id)?.clone();
        let changed = state
            .agents
            .get(&agent_id)
            .is_none_or(|entry| entry.activity != activity);
        state.agents.insert(
            agent_id.clone(),
            RuntimeEntry {
                activity: activity.clone(),
                last_event: Instant::now(),
            },
        );
        changed.then_some((agent_id, activity))
    }

    pub async fn activity(&self, agent_id: &str) -> AgentActivity {
        self.state
            .lock()
            .await
            .agents
            .get(agent_id)
            .map(|entry| entry.activity.clone())
            .unwrap_or(AgentActivity::Idle)
    }

    pub async fn mark_unresponsive(&self, timeout: Duration) -> Vec<String> {
        let now = Instant::now();
        let mut state = self.state.lock().await;
        let mut changed = Vec::new();
        for (agent_id, entry) in &mut state.agents {
            if !matches!(
                entry.activity,
                AgentActivity::Idle | AgentActivity::Unresponsive
            ) && now.saturating_duration_since(entry.last_event) >= timeout
            {
                entry.activity = AgentActivity::Unresponsive;
                changed.push(agent_id.clone());
            }
        }
        changed
    }
}
