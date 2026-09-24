use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
};

use crate::protocol::{
    ResponseMode, RoutingRequest, TriagePayload, TriageReportRequest, TriageVerdict, entity_id,
};
use sqlx::{FromRow, PgPool};

use super::{
    auth::{AgentClaims, authorize_agent_transaction},
    climate::Climate,
    rooms::Rooms,
    routing::{Addressing, HumanMessage, addressing, routing_request},
};

/// 人类消息进入正式 Turn 时给主模型的提示。
const HUMAN_PROMPT_NOTE: &str = "A human is waiting. Read whom they addressed and respond only if this Agent is the intended teammate or the whole group was addressed.";

/// 人类消息那一步的结果。
enum HumanStep {
    Decided(TriageVerdict),
    Route(RoutingRequest),
    RoutedAway,
}

fn verdict(actionable: bool, reason: &str, prompt_note: &str, source: &str) -> TriageVerdict {
    TriageVerdict {
        actionable,
        reason: reason.to_string(),
        prompt_note: prompt_note.to_string(),
        source: source.to_string(),
    }
}

fn decided(verdict: TriageVerdict, model: String) -> TriagePayload {
    TriagePayload {
        verdict: Some(verdict),
        instructions: None,
        input: None,
        model,
        routing: None,
    }
}

const AGENT_LOOP_TRIAGE_INTERVAL: i64 = 8;
pub(super) const AGENT_LOOP_HARD_CAP: i64 = 20;

#[derive(Clone)]
pub struct InboxTriage {
    pool: PgPool,
}

impl InboxTriage {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 为本 Run 的 delivery 构造 triage：能确定时直接给结论，需要模型时给 triage 题；本批人类消息
    /// 全部点名了别人时先给路由题，`routed` 是 Computer 回答后的答案（collaboration.md §8.3）。
    pub async fn payload(
        &self,
        claims: &AgentClaims,
        run_id: &str,
        routed: Option<ResponseMode>,
    ) -> Result<TriagePayload, sqlx::Error> {
        let (persona, role, model) = self.agent_profile(claims).await?;
        let context = self.context(claims, run_id).await?;
        if context
            .unread
            .iter()
            .all(|message| message.message_kind == "system")
        {
            return Ok(decided(
                verdict(
                    false,
                    "unread delivery contains only system messages",
                    "",
                    "system_only",
                ),
                model,
            ));
        }
        let mut unread = context.unread.iter().collect::<Vec<_>>();
        let humans = unread
            .iter()
            .copied()
            .filter(|message| message.author_kind == "user" && message.message_kind != "system")
            .collect::<Vec<_>>();
        if !humans.is_empty() {
            match self.human_step(claims, &humans, routed).await? {
                HumanStep::Decided(verdict) => return Ok(decided(verdict, model)),
                HumanStep::Route(request) => {
                    return Ok(TriagePayload {
                        verdict: None,
                        instructions: None,
                        input: None,
                        model,
                        routing: Some(request),
                    });
                }
                HumanStep::RoutedAway => {
                    unread.retain(|message| message.author_kind != "user");
                    if unread
                        .iter()
                        .all(|message| message.message_kind == "system")
                    {
                        return Ok(decided(
                            verdict(
                                false,
                                "every human message was addressed to other teammates",
                                "",
                                "routing",
                            ),
                            model,
                        ));
                    }
                }
            }
        }
        let real_unread = unread
            .iter()
            .copied()
            .filter(|message| message.message_kind != "system")
            .collect::<Vec<_>>();
        if let Some(verdict) = agent_loop_verdict(&real_unread) {
            return Ok(decided(verdict, model));
        }
        let input = self
            .model_input(claims, &persona, role.as_deref(), &unread, &context.recent)
            .await?;
        Ok(TriagePayload {
            verdict: None,
            instructions: Some(
                "This unread delivery is agent-only. Decide whether it needs a full Agent turn. A specific request for this Agent's decision or action is actionable. If recent context shows a human is still waiting and the unread agent message advances that work, it is actionable. Pure acknowledgements, agreement, repetition, and open-ended agent chatter without concrete work are not actionable. A Room with agent_streak 20 or higher is hard capped: acknowledge it instead of replying. When unsure, prefer actionable. Return only JSON with: {\"actionable\": boolean, \"reason\": string, \"promptNote\": string}. Do not answer the message and do not call tools."
                    .to_string(),
            ),
            input: Some(input),
            model,
            routing: None,
        })
    }

    /// 人类消息那一步：有任何一条需要本 Agent 参与时直接参与；全部点名了别人时，
    /// 没有答案就出路由题，答 `each` 就参与，答 `me` 就把这些消息交还给后续步骤。
    async fn human_step(
        &self,
        claims: &AgentClaims,
        humans: &[&TriageMessage],
        routed: Option<ResponseMode>,
    ) -> Result<HumanStep, sqlx::Error> {
        let room_ids = humans
            .iter()
            .map(|message| message.room_id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let candidates = Rooms::agent_members(&self.pool, &room_ids).await?;
        let empty = Vec::new();
        let mut named = Vec::with_capacity(humans.len());
        for message in humans {
            let room_candidates = candidates.get(&message.room_id).unwrap_or(&empty);
            let address = addressing(
                &HumanMessage {
                    body: &message.body,
                    room_kind: &message.room_kind,
                    quoted_agent_id: message.quoted_agent_id.as_deref(),
                    candidates: room_candidates,
                },
                &claims.sub,
            );
            match address {
                Addressing::Engage => {
                    return Ok(HumanStep::Decided(verdict(
                        true,
                        "unread delivery contains a human message",
                        HUMAN_PROMPT_NOTE,
                        "deterministic",
                    )));
                }
                Addressing::NamesOthers { targets } => {
                    named.push((message.body.as_str(), targets, room_candidates));
                }
            }
        }
        Ok(match routed {
            None => {
                let questions = named
                    .iter()
                    .map(|(body, targets, candidates)| {
                        (*body, targets.as_slice(), candidates.as_slice())
                    })
                    .collect::<Vec<_>>();
                let (instructions, input) = routing_request(&questions);
                HumanStep::Route(RoutingRequest {
                    instructions,
                    input,
                })
            }
            Some(ResponseMode::Each) => HumanStep::Decided(verdict(
                true,
                "the named message is for the whole room",
                HUMAN_PROMPT_NOTE,
                "routing",
            )),
            Some(ResponseMode::Me) => HumanStep::RoutedAway,
        })
    }

    /// triage 模型的输入：persona、私有 Climate、近期上下文与本批未读。
    async fn model_input(
        &self,
        claims: &AgentClaims,
        persona: &str,
        role: Option<&str>,
        unread: &[&TriageMessage],
        recent: &[TriageMessage],
    ) -> Result<String, sqlx::Error> {
        let mut input = format!(
            "Agent persona:\nrole: {}\npersona: {}\n",
            role.unwrap_or("unspecified"),
            persona,
        );
        let participant_ids = unread
            .iter()
            .copied()
            .chain(recent)
            .map(|message| message.author_id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let climates = Climate::for_participants(&self.pool, &claims.sub, &participant_ids).await?;
        if !climates.is_empty() {
            input.push_str(
                "\nPrivate Climate context (this Agent's subjective current impressions):\n",
            );
            for climate in climates {
                input.push_str(&format!(
                    "about_participant_id: {}\naffinity: {}\ntrust: {}\nnote: {}\n\n",
                    climate.about_participant_id,
                    climate.affinity,
                    climate.trust,
                    climate.last_note.as_deref().unwrap_or("none"),
                ));
            }
        }
        if !recent.is_empty() {
            input.push_str("\nRecent posted context:\n");
            for message in recent {
                append_message(&mut input, message);
            }
        }
        input.push_str("\nUnread durable inbox:\n");
        for message in unread {
            append_message(&mut input, message);
        }
        Ok(input)
    }

    pub async fn report(
        &self,
        claims: &AgentClaims,
        request: &TriageReportRequest,
    ) -> Result<(), sqlx::Error> {
        self.record(claims, request).await
    }

    async fn agent_profile(
        &self,
        claims: &AgentClaims,
    ) -> Result<(String, Option<String>, String), sqlx::Error> {
        sqlx::query_as(
            "SELECT profile.persona, profile.role, config.triage_model_id
             FROM collab_agent_profiles profile
             JOIN collab_agent_runtime_configs config ON config.agent_id = profile.agent_id
             WHERE profile.agent_id = $1 AND profile.archived_at IS NULL",
        )
        .bind(&claims.sub)
        .fetch_one(&self.pool)
        .await
    }

    async fn context(
        &self,
        claims: &AgentClaims,
        run_id: &str,
    ) -> Result<TriageContext, sqlx::Error> {
        let unread = sqlx::query_as::<_, TriageMessage>(
            "SELECT m.id, m.room_id, room.kind AS room_kind, m.sequence,
                    m.author_id, author.kind AS author_kind,
                    author.display_name AS author_name, m.kind AS message_kind, m.body,
                    agent_chain.agent_streak, agent_chain.agent_authors,
                    quoted_author.id AS quoted_agent_id
             FROM collab_runs r
             JOIN collab_run_deliveries d ON d.run_id = r.id
             JOIN collab_messages m ON m.room_id = d.room_id
                AND m.sequence BETWEEN d.from_seq AND d.up_to_seq
             JOIN collab_rooms room ON room.id = m.room_id
             JOIN collab_participants author ON author.id = m.author_id
             LEFT JOIN collab_messages quoted
               ON quoted.room_id = m.room_id AND quoted.id = m.quoted_message_id
             LEFT JOIN collab_participants quoted_author
               ON quoted_author.id = quoted.author_id AND quoted_author.kind = 'agent'
             JOIN LATERAL (
                 SELECT COUNT(*) AS agent_streak,
                        COUNT(DISTINCT trailing_message.author_id) AS agent_authors
                 FROM collab_messages trailing_message
                 JOIN collab_participants trailing_author
                   ON trailing_author.id = trailing_message.author_id
                 WHERE trailing_message.room_id = d.room_id
                   AND trailing_message.sequence <= d.up_to_seq
                   AND trailing_message.sequence > GREATEST(COALESCE((
                       SELECT MAX(previous.sequence)
                       FROM collab_messages previous
                       JOIN collab_participants previous_author
                         ON previous_author.id = previous.author_id
                       WHERE previous.room_id = d.room_id
                         AND previous.sequence <= d.up_to_seq
                         AND previous_author.kind <> 'agent'
                   ), 0), room.user_viewed_seq)
                   AND trailing_author.kind = 'agent'
                   AND trailing_message.kind <> 'system'
             ) agent_chain ON TRUE
             WHERE r.id = $1 AND r.agent_id = $2
               AND r.runtime_session_id = $3 AND r.status = 'running'
               AND m.author_id <> $2
             ORDER BY m.room_id, m.sequence",
        )
        .bind(run_id)
        .bind(&claims.sub)
        .bind(&claims.runtime_session_id)
        .fetch_all(&self.pool)
        .await?;
        if unread.is_empty() {
            return Err(sqlx::Error::RowNotFound);
        }
        let recent = sqlx::query_as::<_, TriageMessage>(
            "SELECT message.id, message.room_id, room.kind AS room_kind,
                    message.sequence, message.author_id,
                    author.kind AS author_kind, author.display_name AS author_name,
                    message.kind AS message_kind, message.body,
                    0::BIGINT AS agent_streak, 0::BIGINT AS agent_authors,
                    NULL::TEXT AS quoted_agent_id
             FROM collab_runs run
             JOIN collab_run_deliveries delivery ON delivery.run_id = run.id
             JOIN collab_room_members member
               ON member.room_id = delivery.room_id
              AND member.participant_id = run.agent_id
             JOIN LATERAL (
                 SELECT history.id, history.room_id, history.sequence,
                        history.author_id, history.kind, history.body
                 FROM collab_messages history
                 WHERE history.room_id = delivery.room_id
                   AND history.sequence < delivery.from_seq
                   AND history.created_at >= member.joined_at
                 ORDER BY history.sequence DESC
                 LIMIT 12
             ) message ON TRUE
             JOIN collab_rooms room ON room.id = message.room_id
             JOIN collab_participants author ON author.id = message.author_id
             WHERE run.id = $1 AND run.agent_id = $2
               AND run.runtime_session_id = $3 AND run.status = 'running'
             ORDER BY message.room_id, message.sequence",
        )
        .bind(run_id)
        .bind(&claims.sub)
        .bind(&claims.runtime_session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(TriageContext { unread, recent })
    }

    async fn record(
        &self,
        claims: &AgentClaims,
        request: &TriageReportRequest,
    ) -> Result<(), sqlx::Error> {
        let mut transaction = self.pool.begin().await?;
        authorize_agent_transaction(&mut transaction, claims).await?;
        if !matches!(
            request.verdict.source.as_str(),
            "local_model"
                | "deterministic"
                | "system_only"
                | "agent_dm_engage"
                | "loop_cap"
                | "lap_floor"
                | "routing"
        ) {
            return Err(protocol_error("INVALID_ARGUMENT: invalid triage source"));
        }
        if matches!(
            request.verdict.source.as_str(),
            "system_only" | "loop_cap" | "lap_floor"
        ) && request.verdict.actionable
        {
            return Err(protocol_error(
                "INVALID_ARGUMENT: suppressing triage source cannot be actionable",
            ));
        }
        if request.verdict.source == "agent_dm_engage" && !request.verdict.actionable {
            return Err(protocol_error(
                "INVALID_ARGUMENT: Agent DM engage triage must be actionable",
            ));
        }
        let existing: Option<bool> = sqlx::query_scalar(
            "SELECT actionable FROM collab_triages
             WHERE run_id = $1
             LIMIT 1",
        )
        .bind(&request.run_id)
        .fetch_optional(&mut *transaction)
        .await?;
        if existing.is_some_and(|actionable| actionable != request.verdict.actionable) {
            return Err(protocol_error(
                "CONFLICT: triage verdict is immutable once recorded",
            ));
        }
        let deliveries: Vec<(String, i64, String, String)> = sqlx::query_as(
            "SELECT d.room_id, d.up_to_seq, r.engine_id, r.triage_model_id
             FROM collab_run_deliveries d
             JOIN collab_runs r ON r.id = d.run_id
             WHERE d.run_id = $1 AND r.agent_id = $2
               AND r.runtime_session_id = $3 AND r.status = 'running'
             FOR UPDATE OF d",
        )
        .bind(&request.run_id)
        .bind(&claims.sub)
        .bind(&claims.runtime_session_id)
        .fetch_all(&mut *transaction)
        .await?;
        if deliveries.is_empty() {
            return Err(sqlx::Error::RowNotFound);
        }
        for (room_id, up_to_seq, engine_id, model_id) in deliveries {
            sqlx::query(
                "INSERT INTO collab_triages (
                    id, run_id, agent_id, runtime_session_id, room_id, up_to_seq,
                    actionable, source, reason, prompt_note, engine_id, model_id,
                    input_tokens, output_tokens, latency_ms, response_mode
                 )
                 SELECT $1, $2, $3, $4, $5, $6, $7, $8,
                        $9, $10, $11, $12, $13, $14, $15, $16
                 WHERE NOT EXISTS (
                    SELECT 1 FROM collab_triages
                    WHERE run_id = $2 AND room_id = $5
                 )",
            )
            .bind(entity_id("triage"))
            .bind(&request.run_id)
            .bind(&claims.sub)
            .bind(&claims.runtime_session_id)
            .bind(&room_id)
            .bind(up_to_seq)
            .bind(request.verdict.actionable)
            .bind(&request.verdict.source)
            .bind(&request.verdict.reason)
            .bind(&request.verdict.prompt_note)
            .bind(&engine_id)
            .bind(if request.model.trim().is_empty() {
                &model_id
            } else {
                &request.model
            })
            .bind(request.input_tokens)
            .bind(request.output_tokens)
            .bind(request.latency_ms)
            .bind(request.response_mode.map(ResponseMode::as_str))
            .execute(&mut *transaction)
            .await?;
            if !request.verdict.actionable {
                sqlx::query(
                    "UPDATE collab_run_deliveries
                     SET eligible_reason = 'triage_false',
                         eligible_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
                     WHERE run_id = $1 AND room_id = $2 AND eligible_reason IS NULL",
                )
                .bind(&request.run_id)
                .bind(&room_id)
                .execute(&mut *transaction)
                .await?;
            }
        }
        transaction.commit().await
    }
}

fn agent_loop_verdict(messages: &[&TriageMessage]) -> Option<TriageVerdict> {
    if messages.is_empty()
        || !messages
            .iter()
            .all(|message| message.author_kind == "agent")
    {
        return None;
    }
    let mut latest_by_room = BTreeMap::<&str, (&str, i64, i64)>::new();
    for message in messages {
        latest_by_room
            .entry(&message.room_id)
            .and_modify(|(_, streak, authors)| {
                *streak = (*streak).max(message.agent_streak);
                *authors = (*authors).max(message.agent_authors);
            })
            .or_insert((
                &message.room_kind,
                message.agent_streak,
                message.agent_authors,
            ));
    }
    if latest_by_room
        .values()
        .all(|(_, streak, _)| *streak >= AGENT_LOOP_HARD_CAP)
    {
        return Some(TriageVerdict {
            actionable: false,
            reason: format!(
                "Agent conversation reached the {AGENT_LOOP_HARD_CAP}-message hard loop cap"
            ),
            prompt_note: String::new(),
            source: "loop_cap".to_string(),
        });
    }
    if latest_by_room.values().all(|(room_kind, streak, _)| {
        *room_kind == "direct" && *streak % AGENT_LOOP_TRIAGE_INTERVAL != 0
    }) {
        return Some(TriageVerdict {
            actionable: true,
            reason: format!(
                "Agent-to-Agent Direct Room engages between every {AGENT_LOOP_TRIAGE_INTERVAL}th-message loop check"
            ),
            prompt_note: "A teammate messaged you directly. Reply in that Direct Room.".to_string(),
            source: "agent_dm_engage".to_string(),
        });
    }
    // lap floor（Cumora `triage-core.ts` 的 `pastFloor`）：有人开始第二次发言，说明一整轮已经结束。
    if latest_by_room
        .values()
        .all(|(_, streak, authors)| *streak > *authors)
    {
        return Some(verdict(
            false,
            "every unread room is repeating a full round of agent replies since the last human attention",
            "",
            "lap_floor",
        ));
    }
    None
}

struct TriageContext {
    unread: Vec<TriageMessage>,
    recent: Vec<TriageMessage>,
}

#[derive(FromRow)]
struct TriageMessage {
    id: String,
    room_id: String,
    room_kind: String,
    sequence: i64,
    author_id: String,
    author_kind: String,
    author_name: String,
    message_kind: String,
    body: String,
    /// 自最近一次人类关注（人类消息或用户看到的位置）后的 Agent 消息数。
    agent_streak: i64,
    /// 这些 Agent 消息来自几个不同的 Agent，即“一轮”的长度（collaboration.md §8.3）。
    agent_authors: i64,
    /// 被引用消息的作者，只在作者是 Agent 时有值。
    quoted_agent_id: Option<String>,
}

fn append_message(input: &mut String, message: &TriageMessage) {
    let _ = writeln!(
        input,
        "room_id: {}\nroom_kind: {}\nmessage_id: {}\nmessage_kind: {}\nsequence: {}\nagent_streak: {}\nauthor_id: {}\nauthor_kind: {}\nauthor_name: {}\nbody: {}\n",
        message.room_id,
        message.room_kind,
        message.id,
        message.message_kind,
        message.sequence,
        message.agent_streak,
        message.author_id,
        message.author_kind,
        message.author_name,
        message.body,
    );
}

fn protocol_error(message: &str) -> sqlx::Error {
    sqlx::Error::Protocol(message.to_string())
}

#[cfg(test)]
mod tests {
    use super::{TriageMessage, agent_loop_verdict};

    /// 每条 Agent 消息都来自不同的 Agent，lap floor 不会触发，只检验检查点与硬上限。
    fn agent_message(room_id: &str, room_kind: &str, agent_streak: i64) -> TriageMessage {
        lapping(room_id, room_kind, agent_streak, agent_streak)
    }

    fn lapping(
        room_id: &str,
        room_kind: &str,
        agent_streak: i64,
        agent_authors: i64,
    ) -> TriageMessage {
        TriageMessage {
            id: format!("msg-{agent_streak}"),
            room_id: room_id.to_string(),
            room_kind: room_kind.to_string(),
            sequence: agent_streak,
            author_id: "agent-peer".to_string(),
            author_kind: "agent".to_string(),
            author_name: "Peer".to_string(),
            message_kind: "normal".to_string(),
            body: "hello".to_string(),
            quoted_agent_id: None,
            agent_streak,
            agent_authors,
        }
    }

    /// collaboration.md §8.3、§16 #10：自最近一次人类关注后 Agent 消息数超过发言的 Agent 数，
    /// 且本批每个房间都如此时，确定性跳过；私聊在检查点之间照常参与。
    #[test]
    fn acc_10_a_lapping_agent_run_is_skipped_without_a_model() {
        let one_round = lapping("room-group", "group", 3, 3);
        assert!(agent_loop_verdict(&[&one_round]).is_none());

        let second_lap = lapping("room-group", "group", 4, 3);
        let verdict = agent_loop_verdict(&[&second_lap]).expect("lap floor suppresses");
        assert_eq!(
            (
                verdict.actionable,
                verdict.source.as_str(),
                verdict.reason.as_str()
            ),
            (
                false,
                "lap_floor",
                "every unread room is repeating a full round of agent replies since the last human attention"
            )
        );

        let quiet_room = lapping("room-other", "group", 1, 1);
        assert!(agent_loop_verdict(&[&second_lap, &quiet_room]).is_none());

        let direct_between_checks = lapping("room-direct", "direct", 5, 2);
        assert_eq!(
            agent_loop_verdict(&[&direct_between_checks])
                .unwrap()
                .source,
            "agent_dm_engage"
        );
        let direct_checkpoint = lapping("room-direct", "direct", 8, 2);
        assert_eq!(
            agent_loop_verdict(&[&direct_checkpoint]).unwrap().source,
            "lap_floor"
        );
    }

    #[test]
    fn direct_check_cadence_and_hard_cap_are_independent() {
        let between = agent_message("room-direct", "direct", 7);
        let verdict = agent_loop_verdict(&[&between]).expect("between checks engages");
        assert!(verdict.actionable);
        assert_eq!(verdict.source, "agent_dm_engage");

        let checkpoint = agent_message("room-direct", "direct", 8);
        assert!(agent_loop_verdict(&[&checkpoint]).is_none());

        let capped = agent_message("room-direct", "direct", 20);
        let verdict = agent_loop_verdict(&[&capped]).expect("hard cap suppresses");
        assert!(!verdict.actionable);
        assert_eq!(verdict.source, "loop_cap");
    }

    #[test]
    fn group_uses_model_triage_until_the_deterministic_agent_loop_cap() {
        let slow_group = agent_message("room-group", "group", 8);
        assert!(agent_loop_verdict(&[&slow_group]).is_none());

        let capped_group = agent_message("room-group", "group", 20);
        let verdict = agent_loop_verdict(&[&capped_group]).expect("group cap suppresses");
        assert!(!verdict.actionable);
        assert_eq!(verdict.source, "loop_cap");

        let fresh_other_room = agent_message("room-other", "group", 1);
        assert!(agent_loop_verdict(&[&capped_group, &fresh_other_room]).is_none());
    }
}
