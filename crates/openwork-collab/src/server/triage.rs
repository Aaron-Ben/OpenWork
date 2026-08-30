use crate::protocol::{TriagePayload, TriageReportRequest, TriageVerdict};

use super::{auth::AgentClaims, storage::CollaborationStore};

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
        let messages = self.store.triage_messages(claims, run_id).await?;
        if messages.is_empty() {
            return Ok(TriagePayload {
                verdict: Some(TriageVerdict {
                    actionable: false,
                    reason: "inbox empty".to_string(),
                    prompt_note: String::new(),
                    source: "empty_inbox".to_string(),
                }),
                instructions: None,
                input: None,
                model,
            });
        }
        let mut input = format!(
            "Agent persona:\nrole: {}\nbio: {}\nsystem prompt: {}\n\nUnread durable inbox:\n",
            role.as_deref().unwrap_or("unspecified"),
            bio.as_deref().unwrap_or("unspecified"),
            system_prompt,
        );
        for message in messages {
            input.push_str(&format!(
                "room_id: {}\nmessage_id: {}\nsequence: {}\nauthor: {}\nbody: {}\n\n",
                message.room_id, message.id, message.sequence, message.author_id, message.body,
            ));
        }
        Ok(TriagePayload {
            verdict: None,
            instructions: Some(
                "Decide whether the unread inbox needs a full Agent turn. Return only JSON with exactly: {\"actionable\": boolean, \"reason\": string, \"promptNote\": string}. A direct human request, question, correction, or task is actionable. Pure acknowledgements, duplicate information, and messages that clearly require no response are not actionable. Do not answer the message and do not call tools."
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
