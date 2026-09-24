//! Agent 发消息（collaboration.md §9）：`reply` 在成员超过 2 人的房间里依次检查连发（§9.4）、
//! HELD（§9.1）与逐字重复（§9.2），`dm` 只检查硬上限。检查与写入在同一事务里，房间行已锁。

use sqlx::{Postgres, Transaction};

use crate::protocol::{
    AgentCommandEffect, AgentCommandResponse, AgentCommandResult, QuotedMessageView,
};

use super::{AgentCommands, error, message_effect, success};
use crate::server::{
    agents::Agents,
    auth::AgentClaims,
    coordination::{HeldBinding, HeldReservation},
    messages::{MessageAuthor, Messages},
    rooms::get_or_create_direct_room,
    runs::Runs,
    triage::AGENT_LOOP_HARD_CAP,
};

/// 一次 `reply` 的输入。
pub(super) struct ReplyInput<'a> {
    pub(super) room_id: &'a str,
    pub(super) body: &'a str,
    pub(super) held_token: Option<&'a str>,
    pub(super) quoted_message_id: Option<&'a str>,
    pub(super) request_id: &'a str,
    /// `--continue`：跳过连发与 HELD，不跳过逐字重复。
    pub(super) continuation: bool,
}

/// 发布前三道检查的输入：要发的内容，以及本 Agent 在这个 Run 里对房间的状态。
struct Gates<'a> {
    run_id: &'a str,
    room_id: &'a str,
    body: &'a str,
    held_token: Option<&'a str>,
    request_id: &'a str,
    snapshot_anchor: i64,
    continuation: bool,
}

/// 检查没有通过时给模型的回应，以及 HELD token 是否已被本请求预留。
struct Rejection {
    response: AgentCommandResponse,
    held_reserved: bool,
}

impl AgentCommands {
    /// 返回回应，以及 HELD token 是否被本请求预留（提交后由调用方消费）。
    pub(super) async fn reply(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        run_id: &str,
        claims: &AgentClaims,
        input: ReplyInput<'_>,
    ) -> Result<(AgentCommandResponse, bool), sqlx::Error> {
        let room_id = input.room_id;
        if !Messages::valid_body(input.body) {
            return Ok((error("INVALID_ARGUMENT", "message body is invalid"), false));
        }
        let context = Runs::reply_context_in(transaction, run_id, room_id, &claims.sub).await?;
        let Some((snapshot_anchor, room_kind, member_count)) = context else {
            return Ok((error("NOT_FOUND", "Room is not in the active Run"), false));
        };
        let quoted = match quote(transaction, room_id, input.quoted_message_id).await? {
            Ok(quoted) => quoted,
            Err(response) => return Ok((response, false)),
        };
        if room_kind == "direct" && input.held_token.is_some() {
            let reason = "HELD token is not valid for a Direct Room";
            return Ok((error("INVALID_ARGUMENT", reason), false));
        }
        if Messages::agent_loop_capped_in(transaction, room_id, AGENT_LOOP_HARD_CAP).await? {
            let reason = "Agent conversation reached its deterministic loop cap";
            return Ok((error("LOOP_CAP", reason), false));
        }
        // Cumora 以 `member_count > 2` 为条件：私聊与只有两人的群里，并行打字与重复一句都是正常的。
        let mut held_reserved = false;
        if member_count > 2 {
            let gates = Gates {
                run_id,
                room_id,
                body: input.body,
                held_token: input.held_token,
                request_id: input.request_id,
                snapshot_anchor,
                continuation: input.continuation,
            };
            match self.check_gates(transaction, claims, &gates).await? {
                Ok(reserved) => held_reserved = reserved,
                Err(rejection) => return Ok((rejection.response, rejection.held_reserved)),
            }
        }
        let author = MessageAuthor {
            id: &claims.sub,
            run_id: Some(run_id),
        };
        let message = Messages::insert_in(transaction, room_id, author, input.body, quoted).await?;
        Runs::mark_delivery_action_in(transaction, run_id, room_id).await?;
        let effects = vec![message_effect(&message)];
        let response = AgentCommandResponse {
            result: AgentCommandResult::MessagePublished { message },
            effects,
        };
        Ok((response, held_reserved))
    }

    /// 依次检查连发（§9.4）、HELD（§9.1）与逐字重复（§9.2）；`--continue` 跳过前两道。通过时返回
    /// HELD token 是否已被本请求预留。
    async fn check_gates(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        claims: &AgentClaims,
        gates: &Gates<'_>,
    ) -> Result<Result<bool, Rejection>, sqlx::Error> {
        let mut held_reserved = false;
        if !gates.continuation {
            let monologue =
                Messages::monologue_in(transaction, gates.room_id, &claims.sub, gates.run_id)
                    .await?;
            if let Some(reason) = monologue {
                return Ok(Err(rejected("MONOLOGUE", &reason, false)));
            }
            match self.check_freshness(transaction, claims, gates).await? {
                Ok(reserved) => held_reserved = reserved,
                Err(rejection) => return Ok(Err(rejection)),
            }
        }
        let duplicate = Messages::duplicate_of_last_peer_in(
            transaction,
            gates.room_id,
            &claims.sub,
            gates.body,
        )
        .await?;
        Ok(match duplicate {
            Some(reason) => Err(rejected("DUPLICATE", &reason, held_reserved)),
            None => Ok(held_reserved),
        })
    }

    /// HELD（§9.1）：带 token 时核对并预留 token，房间在 token 之后又变了就再 HELD 一次；不带 token 时
    /// 与 seen sequence 比较。通过时返回 token 是否已被本请求预留。
    async fn check_freshness(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        claims: &AgentClaims,
        check: &Gates<'_>,
    ) -> Result<Result<bool, Rejection>, sqlx::Error> {
        let room_id = check.room_id;
        let Some(token) = check.held_token else {
            let seen_baseline = match self.coordination.get_seen(&claims.sub, room_id).await {
                Ok(Some(sequence)) if sequence > 0 => sequence,
                Ok(_) => check.snapshot_anchor,
                Err(error) => {
                    tracing::warn!(%error, %room_id, "seen lookup failed; using durable snapshot");
                    check.snapshot_anchor
                }
            };
            return self
                .hold_if_changed(transaction, claims, check, seen_baseline, false)
                .await;
        };
        let reservation = self
            .coordination
            .reserve_held(&claims.sub, room_id, token, check.request_id)
            .await;
        let binding = match reservation {
            Ok(HeldReservation::Reserved(binding)) => binding,
            Ok(HeldReservation::Missing) => {
                return Ok(Err(rejected(
                    "HELD",
                    "retry token is invalid or expired",
                    false,
                )));
            }
            Ok(HeldReservation::OwnedByAnotherRequest) => {
                return Ok(Err(rejected(
                    "HELD",
                    "retry token belongs to another request",
                    false,
                )));
            }
            Err(redis_error) => {
                tracing::warn!(%redis_error, %room_id, "HELD token reservation failed closed");
                let reason = "coordination is temporarily unavailable";
                return Ok(Err(rejected("RATE_LIMITED", reason, false)));
            }
        };
        if binding.agent_id != claims.sub
            || binding.run_id != check.run_id
            || binding.room_id != room_id
            || binding.runtime_session_id != claims.runtime_session_id
        {
            return Ok(Err(rejected(
                "HELD",
                "retry token does not match this Run",
                true,
            )));
        }
        self.hold_if_changed(transaction, claims, check, binding.shown_peer_max, true)
            .await
    }

    /// `seen_baseline` 之后有别人的新消息时 HELD，否则通过。
    async fn hold_if_changed(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        claims: &AgentClaims,
        check: &Gates<'_>,
        seen_baseline: i64,
        held_reserved: bool,
    ) -> Result<Result<bool, Rejection>, sqlx::Error> {
        let peer_max =
            Messages::peer_max_in(transaction, check.room_id, &claims.sub, seen_baseline).await?;
        let Some(peer_max) = peer_max else {
            return Ok(Ok(held_reserved));
        };
        let response = self
            .hold_reply(transaction, claims, check, seen_baseline, peer_max)
            .await?;
        Ok(Err(Rejection {
            response,
            held_reserved,
        }))
    }

    /// 列出 Agent 没看过的消息（最多 8 条），签发 HELD token，并把 seen sequence 推进到列出的最后一条。
    async fn hold_reply(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        claims: &AgentClaims,
        check: &Gates<'_>,
        seen_baseline: i64,
        peer_max: i64,
    ) -> Result<AgentCommandResponse, sqlx::Error> {
        let room_id = check.room_id;
        let messages =
            Messages::between_in(transaction, room_id, seen_baseline, peer_max, &claims.sub)
                .await?;
        let shown_peer_max = messages
            .last()
            .map(|message| message.sequence)
            .ok_or(sqlx::Error::RowNotFound)?;
        let binding = HeldBinding {
            agent_id: claims.sub.clone(),
            run_id: check.run_id.to_string(),
            room_id: room_id.to_string(),
            runtime_session_id: claims.runtime_session_id.clone(),
            shown_peer_max,
        };
        let retry_token = match self.coordination.issue_held(&binding).await {
            Ok(token) => token,
            Err(redis_error) => {
                tracing::warn!(%redis_error, %room_id, "HELD token issuance failed closed");
                let reason = "coordination is temporarily unavailable";
                return Ok(error("RATE_LIMITED", reason));
            }
        };
        if let Err(error) = self
            .coordination
            .record_seen(&claims.sub, room_id, shown_peer_max)
            .await
        {
            tracing::warn!(%error, %room_id, "HELD seen update failed open");
        }
        Ok(success(AgentCommandResult::Held {
            room_id: room_id.to_string(),
            retry_token,
            messages,
        }))
    }
}

/// `--quote` 的目标；不在本房间时返回给模型的 `NOT_FOUND`，不静默发布无引用的回复（§9.3）。
async fn quote(
    transaction: &mut Transaction<'_, Postgres>,
    room_id: &str,
    quoted_message_id: Option<&str>,
) -> Result<Result<Option<QuotedMessageView>, AgentCommandResponse>, sqlx::Error> {
    let Some(quoted_id) = quoted_message_id else {
        return Ok(Ok(None));
    };
    Ok(
        match Messages::quote_in(transaction, room_id, quoted_id).await? {
            Some(quoted) => Ok(Some(quoted)),
            None => Err(error(
                "NOT_FOUND",
                &Messages::quote_not_found(quoted_id, room_id),
            )),
        },
    )
}

fn rejected(code: &str, message: &str, held_reserved: bool) -> Rejection {
    Rejection {
        response: error(code, message),
        held_reserved,
    }
}

/// `dm`：私聊只有两个成员，不做连发、HELD 与逐字重复检查（§9）。
pub(super) async fn direct_message(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
    claims: &AgentClaims,
    participant_id: &str,
    body: &str,
) -> Result<AgentCommandResponse, sqlx::Error> {
    if participant_id == claims.sub {
        return Ok(error("INVALID_ARGUMENT", "cannot DM yourself"));
    }
    if !Messages::valid_body(body) {
        return Ok(error("INVALID_ARGUMENT", "message body is invalid"));
    }
    let participant_active = Agents::is_active_participant_in(transaction, participant_id).await?;
    if !participant_active {
        return Ok(error(
            "NOT_FOUND",
            "participant does not exist or is archived",
        ));
    }
    let (room_id, _) =
        get_or_create_direct_room(transaction, &claims.sub, participant_id, &claims.sub).await?;
    if Messages::agent_loop_capped_in(transaction, &room_id, AGENT_LOOP_HARD_CAP).await? {
        return Ok(error(
            "LOOP_CAP",
            "Agent conversation reached its deterministic loop cap",
        ));
    }
    let author = MessageAuthor {
        id: &claims.sub,
        run_id: Some(run_id),
    };
    let message = Messages::insert_in(transaction, &room_id, author, body, None).await?;
    Runs::mark_delivery_action_in(transaction, run_id, &room_id).await?;
    Ok(AgentCommandResponse {
        result: AgentCommandResult::DirectMessageSent {
            room_id: room_id.clone(),
            message: message.clone(),
        },
        effects: vec![
            AgentCommandEffect::DirectRoomOpened {
                room_id: room_id.clone(),
                participant_id: participant_id.to_string(),
            },
            message_effect(&message),
        ],
    })
}
