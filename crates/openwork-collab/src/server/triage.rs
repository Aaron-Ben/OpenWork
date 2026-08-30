use std::fmt::Write as _;

use crate::protocol::{TriagePayload, TriageReportRequest, TriageVerdict};

use super::{
    auth::AgentClaims,
    storage::{CollaborationStore, TriageMessage},
};

#[derive(Clone)]
pub struct InboxTriage {
    store: CollaborationStore,
}

impl InboxTriage {
    pub fn new(store: CollaborationStore) -> Self {
        Self { store }
    }

    pub async fn payload(
        &self,
        claims: &AgentClaims,
        run_id: &str,
    ) -> Result<TriagePayload, sqlx::Error> {
        let (system_prompt, role, bio, model) = self.store.agent_triage_profile(claims).await?;
        let context = self.store.triage_context(claims, run_id).await?;
        if context
            .unread
            .iter()
            .all(|message| message.message_kind == "system")
        {
            return Ok(TriagePayload {
                verdict: Some(TriageVerdict {
                    actionable: false,
                    reason: "unread delivery contains only system messages".to_string(),
                    prompt_note: String::new(),
                    source: "system_only".to_string(),
                }),
                instructions: None,
                input: None,
                model,
            });
        }
        if context
            .unread
            .iter()
            .any(|message| message.author_kind == "user")
        {
            return Ok(TriagePayload {
                verdict: Some(TriageVerdict {
                    actionable: true,
                    reason: "unread delivery contains a human message".to_string(),
                    prompt_note: "A human is waiting. Read whom they addressed and respond only if this Agent is the intended teammate or the whole group was addressed.".to_string(),
                    source: "deterministic".to_string(),
                }),
                instructions: None,
                input: None,
                model,
            });
        }
        let mut input = format!(
            "Agent persona:\nrole: {}\nbio: {}\nsystem prompt: {}\n",
            role.as_deref().unwrap_or("unspecified"),
            bio.as_deref().unwrap_or("unspecified"),
            system_prompt,
        );
        if !context.recent.is_empty() {
            input.push_str("\nRecent posted context:\n");
            for message in &context.recent {
                append_message(&mut input, message);
            }
        }
        input.push_str("\nUnread durable inbox:\n");
        for message in &context.unread {
            append_message(&mut input, message);
        }
        Ok(TriagePayload {
            verdict: None,
            instructions: Some(
                "This unread delivery is agent-only. Decide whether it needs a full Agent turn. A specific request for this Agent's decision or action is actionable. If recent context shows a human is still waiting and the unread agent message advances that work, it is actionable. Pure acknowledgements, agreement, repetition, and open-ended agent chatter without concrete work are not actionable. When unsure, prefer actionable. Return only JSON with: {\"actionable\": boolean, \"reason\": string, \"promptNote\": string}. Do not answer the message and do not call tools."
                    .to_string(),
            ),
            input: Some(input),
            model,
        })
    }

    pub async fn report(
        &self,
        claims: &AgentClaims,
        request: &TriageReportRequest,
    ) -> Result<(), sqlx::Error> {
        self.store.record_triage(claims, request).await
    }
}

fn append_message(input: &mut String, message: &TriageMessage) {
    let _ = writeln!(
        input,
        "room_id: {}\nroom_kind: {}\nmessage_id: {}\nmessage_kind: {}\nsequence: {}\nauthor_id: {}\nauthor_kind: {}\nauthor_name: {}\nbody: {}\n",
        message.room_id,
        message.room_kind,
        message.id,
        message.message_kind,
        message.sequence,
        message.author_id,
        message.author_kind,
        message.author_name,
        message.body,
    );
}
