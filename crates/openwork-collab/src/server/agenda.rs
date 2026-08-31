use std::fmt::Write as _;

use crate::protocol::{
    AgendaCandidate, AgendaCandidateSet, AgendaDecision, AgendaDecisionRequest,
    AgendaDecisionResponse, AgendaFocus, AgendaPayload, MessageView, TriggerEnvelope, entity_id,
};
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;

use super::{
    auth::{AgentClaims, SigningKey},
    coordination::Coordination,
};

const CANDIDATE_LIMIT: i64 = 20;
const RECENT_MESSAGE_LIMIT: i64 = 8;
const CANDIDATE_TTL_SECONDS: i64 = 5 * 60;
const STALL_MIN_SECONDS: i64 = 5 * 60;
const STALL_MAX_SECONDS: i64 = 6 * 60 * 60;

#[derive(Clone)]
pub struct Agenda {
    pool: PgPool,
    coordination: Coordination,
    signing_key: SigningKey,
}

impl Agenda {
    pub fn new(pool: PgPool, coordination: Coordination, signing_key: SigningKey) -> Self {
        Self {
            pool,
            coordination,
            signing_key,
        }
    }

    pub async fn payload(&self, claims: &AgentClaims) -> Result<AgendaPayload, sqlx::Error> {
        if !self
            .coordination
            .agenda_allowed(&claims.sub)
            .await
            .map_err(coordination_error)?
        {
            return self.signed_payload(claims, Vec::new());
        }
        let mut candidates = self.card_candidates(&claims.sub).await?;
        if candidates.len() < CANDIDATE_LIMIT as usize {
            let remaining = CANDIDATE_LIMIT - candidates.len() as i64;
            candidates.extend(self.stalled_candidates(&claims.sub, remaining).await?);
        }
        self.signed_payload(claims, candidates)
    }

    pub async fn decide(
        &self,
        claims: &AgentClaims,
        request: AgendaDecisionRequest,
    ) -> Result<AgendaDecisionResponse, sqlx::Error> {
        self.signing_key
            .verify_agenda_candidates(&request.candidate_set)
            .map_err(auth_error)?;
        let now = OffsetDateTime::now_utc().unix_timestamp();
        if request.candidate_set.agent_id != claims.sub
            || request.candidate_set.runtime_session_id != claims.runtime_session_id
            || request.candidate_set.expires_at <= now
            || request.candidate_set.issued_at > now + 30
        {
            return Err(protocol_error(
                "CONFLICT: agenda candidate set is stale or fenced",
            ));
        }
        if request.model.trim().is_empty()
            || request.input_tokens < 0
            || request.output_tokens < 0
            || request.latency_ms < 0
        {
            return Err(protocol_error(
                "INVALID_ARGUMENT: invalid agenda model report",
            ));
        }
        match request.decision {
            AgendaDecision::Decline { reason } => {
                if reason.trim().is_empty() {
                    return Err(protocol_error(
                        "INVALID_ARGUMENT: agenda decline reason is empty",
                    ));
                }
                let declines = self
                    .coordination
                    .record_agenda_decline(&claims.sub)
                    .await
                    .map_err(coordination_error)?;
                self.record_decision(
                    claims,
                    "agenda.declined",
                    serde_json::json!({
                        "candidateSetId": request.candidate_set.id,
                        "reason": reason,
                        "declines": declines,
                        "model": request.model,
                        "inputTokens": request.input_tokens,
                        "outputTokens": request.output_tokens,
                        "latencyMs": request.latency_ms,
                    }),
                )
                .await?;
                Ok(AgendaDecisionResponse {
                    trigger: None,
                    focused_brief: None,
                })
            }
            AgendaDecision::Act {
                candidate_id,
                reason,
            } => {
                if reason.trim().is_empty() {
                    return Err(protocol_error(
                        "INVALID_ARGUMENT: agenda act reason is empty",
                    ));
                }
                let candidate = request
                    .candidate_set
                    .candidates
                    .iter()
                    .find(|candidate| candidate_id_of(candidate) == candidate_id)
                    .ok_or_else(|| {
                        protocol_error("INVALID_ARGUMENT: candidate id is outside the signed set")
                    })?;
                let focus = self.revalidate(claims, candidate, reason.trim()).await?;
                if !self
                    .coordination
                    .claim_agenda_rate(&claims.sub)
                    .await
                    .map_err(coordination_error)?
                {
                    return Ok(AgendaDecisionResponse {
                        trigger: None,
                        focused_brief: None,
                    });
                }
                if matches!(candidate, AgendaCandidate::StalledRoom { .. })
                    && let Some(room_id) = focus.room_id.as_deref()
                    && !self
                        .coordination
                        .claim_room_nudge(room_id)
                        .await
                        .map_err(coordination_error)?
                {
                    return Ok(AgendaDecisionResponse {
                        trigger: None,
                        focused_brief: None,
                    });
                }
                let _ = self
                    .coordination
                    .reset_agenda_declines(std::slice::from_ref(&claims.sub))
                    .await;
                let mut trigger = TriggerEnvelope {
                    dispatch_id: entity_id("run"),
                    agent_id: claims.sub.clone(),
                    runtime_session_id: claims.runtime_session_id.clone(),
                    trigger: "agenda".to_string(),
                    deliveries: Vec::new(),
                    agenda_focus: Some(focus.clone()),
                    carried_over: false,
                    issued_at: now,
                    expires_at: now + CANDIDATE_TTL_SECONDS,
                    signature: String::new(),
                };
                self.signing_key
                    .sign_trigger(&mut trigger)
                    .map_err(auth_error)?;
                let brief = focused_brief(candidate, &focus);
                self.record_decision(
                    claims,
                    "agenda.dispatched",
                    serde_json::json!({
                        "candidateSetId": request.candidate_set.id,
                        "candidateId": candidate_id,
                        "runId": trigger.dispatch_id,
                        "roomId": focus.room_id,
                        "cardId": focus.card_id,
                        "reason": focus.reason,
                        "model": request.model,
                        "inputTokens": request.input_tokens,
                        "outputTokens": request.output_tokens,
                        "latencyMs": request.latency_ms,
                    }),
                )
                .await?;
                Ok(AgendaDecisionResponse {
                    trigger: Some(trigger),
                    focused_brief: Some(brief),
                })
            }
        }
    }

    fn signed_payload(
        &self,
        claims: &AgentClaims,
        candidates: Vec<AgendaCandidate>,
    ) -> Result<AgendaPayload, sqlx::Error> {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let mut candidate_set = AgendaCandidateSet {
            id: entity_id("agenda"),
            agent_id: claims.sub.clone(),
            runtime_session_id: claims.runtime_session_id.clone(),
            candidates,
            issued_at: now,
            expires_at: now + CANDIDATE_TTL_SECONDS,
            signature: String::new(),
        };
        self.signing_key
            .sign_agenda_candidates(&mut candidate_set)
            .map_err(auth_error)?;
        let classify_prompt = classify_prompt(&candidate_set.candidates);
        Ok(AgendaPayload {
            candidate_set,
            classify_prompt,
        })
    }

    async fn card_candidates(&self, agent_id: &str) -> Result<Vec<AgendaCandidate>, sqlx::Error> {
        let rows = sqlx::query_as::<_, CardCandidateRow>(
            "SELECT card.id AS card_id, card.title,
                    board_column.title AS column_title,
                    'assigned'::TEXT AS assignment,
                    floor(extract(epoch FROM card.updated_at))::BIGINT AS updated_at
             FROM collab_cards card
             JOIN collab_board_columns board_column
               ON board_column.id = card.column_id AND NOT board_column.is_terminal
             WHERE card.assignee_id = $1
             ORDER BY card.updated_at, card.id
             LIMIT $2",
        )
        .bind(agent_id)
        .bind(CANDIDATE_LIMIT)
        .fetch_all(&self.pool)
        .await?;
        let mut candidates = Vec::with_capacity(rows.len());
        for row in rows {
            candidates.push(AgendaCandidate::AssignedCard {
                candidate_id: format!("card:{}:{}", row.card_id, row.updated_at),
                card_id: row.card_id,
                room_id: None,
                title: row.title,
                column: row.column_title,
                assignment: row.assignment,
                updated_at: row.updated_at,
                room_sequence: None,
                recent_context: Vec::new(),
            });
        }
        Ok(candidates)
    }

    async fn stalled_candidates(
        &self,
        agent_id: &str,
        limit: i64,
    ) -> Result<Vec<AgendaCandidate>, sqlx::Error> {
        if limit <= 0 {
            return Ok(Vec::new());
        }
        let rows = sqlx::query_as::<_, StalledRoomRow>(
            "SELECT room.id AS room_id, room.next_seq AS last_sequence,
                    floor(extract(epoch FROM room.last_message_at))::BIGINT AS last_activity_at
             FROM collab_rooms room
             JOIN collab_room_members member
               ON member.room_id = room.id AND member.participant_id = $1
             WHERE room.last_message_at <= (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
                       - ($2::double precision * INTERVAL '1 second')
               AND room.last_message_at >= (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
                       - ($3::double precision * INTERVAL '1 second')
             ORDER BY room.last_message_at DESC, room.id
             LIMIT $4",
        )
        .bind(agent_id)
        .bind(STALL_MIN_SECONDS as f64)
        .bind(STALL_MAX_SECONDS as f64)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        let mut candidates = Vec::with_capacity(rows.len());
        for row in rows {
            let recent_context = self.recent_messages(&row.room_id).await?;
            candidates.push(AgendaCandidate::StalledRoom {
                candidate_id: format!("room:{}:{}", row.room_id, row.last_sequence),
                room_id: row.room_id,
                last_sequence: row.last_sequence,
                last_activity_at: row.last_activity_at,
                open_cards: Vec::new(),
                recent_context,
            });
        }
        Ok(candidates)
    }

    async fn recent_messages(&self, room_id: &str) -> Result<Vec<MessageView>, sqlx::Error> {
        let rows = sqlx::query_as::<_, MessageRow>(
            "SELECT id, room_id, sequence, author_id, body
             FROM (
                 SELECT id, room_id, sequence, author_id, body
                 FROM collab_messages WHERE room_id = $1
                 ORDER BY sequence DESC LIMIT $2
             ) recent ORDER BY sequence",
        )
        .bind(room_id)
        .bind(RECENT_MESSAGE_LIMIT)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(MessageView::from).collect())
    }

    async fn revalidate(
        &self,
        claims: &AgentClaims,
        candidate: &AgendaCandidate,
        reason: &str,
    ) -> Result<AgendaFocus, sqlx::Error> {
        match candidate {
            AgendaCandidate::AssignedCard {
                card_id,
                updated_at,
                ..
            } => {
                let current: Option<i64> = sqlx::query_scalar(
                    "SELECT floor(extract(epoch FROM card.updated_at))::BIGINT
                     FROM collab_cards card
                     JOIN collab_board_columns board_column
                       ON board_column.id = card.column_id AND NOT board_column.is_terminal
                     WHERE card.id = $1 AND card.assignee_id = $2",
                )
                .bind(card_id)
                .bind(&claims.sub)
                .fetch_optional(&self.pool)
                .await?;
                let Some(current_updated) = current else {
                    return Err(protocol_error(
                        "CONFLICT: agenda card is no longer actionable",
                    ));
                };
                if current_updated != *updated_at {
                    return Err(protocol_error(
                        "CONFLICT: agenda card changed after classification",
                    ));
                }
                Ok(AgendaFocus {
                    room_id: None,
                    card_id: Some(card_id.clone()),
                    room_sequence: None,
                    reason: reason.to_string(),
                })
            }
            AgendaCandidate::StalledRoom {
                room_id,
                last_sequence,
                ..
            } => {
                let valid: bool = sqlx::query_scalar(
                    "SELECT EXISTS(
                        SELECT 1 FROM collab_rooms room
                        JOIN collab_room_members member
                          ON member.room_id = room.id AND member.participant_id = $2
                        WHERE room.id = $1 AND room.next_seq = $3
                          AND room.last_message_at <= (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
                              - ($4::double precision * INTERVAL '1 second')
                          AND room.last_message_at >= (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
                              - ($5::double precision * INTERVAL '1 second')
                     )",
                )
                .bind(room_id)
                .bind(&claims.sub)
                .bind(last_sequence)
                .bind(STALL_MIN_SECONDS as f64)
                .bind(STALL_MAX_SECONDS as f64)
                .fetch_one(&self.pool)
                .await?;
                if !valid {
                    return Err(protocol_error(
                        "CONFLICT: stalled room changed after classification",
                    ));
                }
                Ok(AgendaFocus {
                    room_id: Some(room_id.clone()),
                    card_id: None,
                    room_sequence: Some(*last_sequence),
                    reason: reason.to_string(),
                })
            }
        }
    }

    async fn record_decision(
        &self,
        claims: &AgentClaims,
        kind: &str,
        payload: serde_json::Value,
    ) -> Result<(), sqlx::Error> {
        tracing::info!(
            agent_id = claims.sub,
            runtime_session_id = claims.runtime_session_id,
            event = kind,
            payload = %payload,
            "Agenda decision"
        );
        Ok(())
    }
}

fn candidate_id_of(candidate: &AgendaCandidate) -> &str {
    match candidate {
        AgendaCandidate::AssignedCard { candidate_id, .. }
        | AgendaCandidate::StalledRoom { candidate_id, .. } => candidate_id,
    }
}

fn classify_prompt(candidates: &[AgendaCandidate]) -> String {
    if candidates.is_empty() {
        return String::new();
    }
    let encoded =
        serde_json::to_string_pretty(candidates).expect("AgendaCandidate is serializable");
    format!(
        "Decide whether exactly one proactive collaboration candidate deserves a main OpenCode turn now.\n\
         Return exactly one JSON object and no markdown or prose.\n\
         Act: {{\"decision\":\"act\",\"candidateId\":\"one exact candidateId below\",\"reason\":\"specific reason\"}}\n\
         Decline: {{\"decision\":\"decline\",\"reason\":\"specific reason\"}}\n\
         A quiet room alone is not actionable. Do not revive concluded conversation.\n\
         Candidates:\n{encoded}"
    )
}

fn focused_brief(candidate: &AgendaCandidate, focus: &AgendaFocus) -> String {
    let mut brief = format!(
        "Trigger: agenda\nFocus room: {}\nFocus card: {}\nWhy now: {}\n",
        focus.room_id.as_deref().unwrap_or("none"),
        focus.card_id.as_deref().unwrap_or("none"),
        focus.reason
    );
    brief.push_str("Recent published state:\n");
    let recent = match candidate {
        AgendaCandidate::AssignedCard { recent_context, .. }
        | AgendaCandidate::StalledRoom { recent_context, .. } => recent_context,
    };
    for message in recent {
        let _ = writeln!(
            brief,
            "[{}] {}: {}",
            message.sequence, message.author_id, message.body
        );
    }
    brief.push_str(
        "Required discipline: inspect current state with openwork CLI before publishing. Assistant text alone is not sent.",
    );
    brief
}

#[derive(FromRow)]
struct CardCandidateRow {
    card_id: String,
    title: String,
    column_title: String,
    assignment: String,
    updated_at: i64,
}

#[derive(FromRow)]
struct StalledRoomRow {
    room_id: String,
    last_sequence: i64,
    last_activity_at: i64,
}

#[derive(FromRow)]
struct MessageRow {
    id: String,
    room_id: String,
    sequence: i64,
    author_id: String,
    body: String,
}

impl From<MessageRow> for MessageView {
    fn from(row: MessageRow) -> Self {
        Self {
            id: row.id,
            room_id: row.room_id,
            sequence: row.sequence,
            author_id: row.author_id,
            body: row.body,
        }
    }
}

fn protocol_error(message: &str) -> sqlx::Error {
    sqlx::Error::Protocol(message.to_string())
}

fn coordination_error(error: redis::RedisError) -> sqlx::Error {
    protocol_error(&format!("COORDINATION: agenda is unavailable: {error}"))
}

fn auth_error(error: super::auth::AuthError) -> sqlx::Error {
    protocol_error(&format!("UNAUTHENTICATED: {error}"))
}
