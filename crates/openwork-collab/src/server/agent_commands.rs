use std::fmt::Write as _;

use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgPool, Postgres, Transaction};

use crate::protocol::{
    AgentCommand, AgentCommandEffect, AgentCommandRequest, AgentCommandResponse,
    AgentCommandResult, MessageView, ParticipantView, entity_id,
};

use super::{
    agents::Agents,
    auth::{AgentClaims, authorize_agent_transaction},
    board::{Board, BoardOperationError},
    climate::{Climate, ClimateOperationError},
    coordination::{Coordination, HeldBinding},
    messages::Messages,
    rooms::{Rooms, get_or_create_direct_room},
};

#[derive(Clone)]
pub(crate) struct AgentCommands {
    pool: PgPool,
    coordination: Coordination,
}

impl AgentCommands {
    pub(crate) fn new(pool: PgPool, coordination: Coordination) -> Self {
        Self { pool, coordination }
    }

    pub(crate) async fn execute(
        &self,
        claims: &AgentClaims,
        request: AgentCommandRequest,
    ) -> Result<AgentCommandResponse, sqlx::Error> {
        if !valid_request_id(&request.request_id) {
            return Ok(error("INVALID_ARGUMENT", "invalid request id"));
        }
        let mut transaction = self.pool.begin().await?;
        authorize_agent_transaction(&mut transaction, claims).await?;
        let mutating = request.command.is_mutating();
        let run_id = if request.command.requires_active_run() {
            let Some(run_id) = active_run(&mut transaction, claims).await? else {
                return Ok(error("UNAUTHENTICATED", "no active Run for this Agent"));
            };
            Some(run_id)
        } else {
            None
        };
        if mutating {
            let run_id = run_id
                .as_deref()
                .expect("every mutating AgentCommand requires an active Run");
            let semantic_hash = digest(
                &serde_json::to_vec(&request.command).expect("AgentCommand is serializable"),
            );
            if let Some(result) = reserve_request(
                &mut transaction,
                run_id,
                claims,
                &request.request_id,
                &semantic_hash,
            )
            .await?
            {
                transaction.commit().await?;
                return Ok(result);
            }
        }

        let response = match request.command {
            AgentCommand::Inbox => {
                current_inbox(
                    &mut transaction,
                    run_id.as_deref().expect("Inbox requires an active Run"),
                    claims,
                )
                .await?
            }
            AgentCommand::Rooms => success(AgentCommandResult::Rooms {
                rooms: Rooms::list_for_agent_in(&mut transaction, &claims.sub).await?,
            }),
            AgentCommand::Messages { room_id, tail } => success(AgentCommandResult::Messages {
                messages: Messages::list_for_agent_in(
                    &mut transaction,
                    &claims.sub,
                    &room_id,
                    tail,
                )
                .await?,
                room_id,
            }),
            AgentCommand::Members { room_id } => success(AgentCommandResult::Members {
                members: Rooms::list_members_for_agent_in(&mut transaction, &claims.sub, &room_id)
                    .await?,
                room_id,
            }),
            AgentCommand::Participants => success(AgentCommandResult::Participants {
                participants: Agents::active_participants_in(&mut transaction).await?,
            }),
            AgentCommand::Glance { room_id } => {
                self.glance(
                    &mut transaction,
                    run_id.as_deref().expect("Glance requires an active Run"),
                    claims,
                    &room_id,
                )
                .await?
            }
            AgentCommand::Reply {
                room_id,
                body,
                held_token,
            } => {
                self.reply(
                    &mut transaction,
                    run_id.as_deref().expect("Reply requires an active Run"),
                    claims,
                    &room_id,
                    &body,
                    held_token.as_deref(),
                )
                .await?
            }
            AgentCommand::Ack { room_id } => {
                ack(
                    &mut transaction,
                    run_id.as_deref().expect("Ack requires an active Run"),
                    &room_id,
                )
                .await?
            }
            AgentCommand::DirectMessage {
                participant_id,
                body,
            } => {
                direct_message(
                    &mut transaction,
                    run_id
                        .as_deref()
                        .expect("DirectMessage requires an active Run"),
                    claims,
                    &participant_id,
                    &body,
                )
                .await?
            }
            AgentCommand::ClimateShow { participant_id } => success(AgentCommandResult::Climates {
                climates: Climate::list_owned_in(
                    &mut transaction,
                    &claims.sub,
                    participant_id.as_deref(),
                )
                .await?,
            }),
            AgentCommand::ClimateNote {
                participant_id,
                affinity,
                trust,
                note,
            } => match Climate::overwrite_in(
                &mut transaction,
                &claims.sub,
                &participant_id,
                affinity,
                trust,
                &note,
            )
            .await
            {
                Ok(climate) => AgentCommandResponse {
                    result: AgentCommandResult::Climate { climate },
                    effects: vec![AgentCommandEffect::ClimateUpdated {
                        about_participant_id: participant_id,
                    }],
                },
                Err(error) => climate_failure(error)?,
            },
            AgentCommand::BoardList => success(AgentCommandResult::Boards {
                boards: Board::list_in(&mut transaction).await?,
            }),
            AgentCommand::BoardShow { board_id } => {
                match Board::get_in(&mut transaction, &board_id).await {
                    Ok(board) => success(AgentCommandResult::Board { board }),
                    Err(sqlx::Error::RowNotFound) => error("NOT_FOUND", "Board does not exist"),
                    Err(error) => return Err(error),
                }
            }
            AgentCommand::CardList { board_id } => success(AgentCommandResult::Cards {
                cards: Board::list_cards_in(&mut transaction, board_id.as_deref()).await?,
            }),
            AgentCommand::CardShow { card_id } => {
                match Board::get_card_in(&mut transaction, &card_id).await {
                    Ok(card) => success(AgentCommandResult::Card { card }),
                    Err(error) => board_failure(error)?,
                }
            }
            AgentCommand::CardCreate {
                board_id,
                column_id,
                title,
                description,
                assignee_id,
            } => {
                match Board::create_card_in(
                    &mut transaction,
                    &board_id,
                    &column_id,
                    &title,
                    description.as_deref(),
                    assignee_id.as_deref(),
                    &claims.sub,
                )
                .await
                {
                    Ok(card) => AgentCommandResponse {
                        result: AgentCommandResult::Card { card: card.clone() },
                        effects: vec![AgentCommandEffect::CardCreated {
                            board_id,
                            card_id: card.id,
                        }],
                    },
                    Err(error) => board_failure(error)?,
                }
            }
            AgentCommand::CardClaim { card_id } => {
                match Board::claim_card_in(&mut transaction, &card_id, &claims.sub).await {
                    Ok(card) => AgentCommandResponse {
                        result: AgentCommandResult::Card { card },
                        effects: vec![AgentCommandEffect::CardAssigned {
                            card_id,
                            assignee_id: claims.sub.clone(),
                        }],
                    },
                    Err(error) => board_failure(error)?,
                }
            }
            AgentCommand::CardAssign {
                card_id,
                assignee_id,
            } => {
                match Board::assign_card_in(&mut transaction, &card_id, Some(&assignee_id)).await {
                    Ok(card) => AgentCommandResponse {
                        result: AgentCommandResult::Card { card },
                        effects: vec![AgentCommandEffect::CardAssigned {
                            card_id,
                            assignee_id,
                        }],
                    },
                    Err(error) => board_failure(error)?,
                }
            }
            AgentCommand::CardUpdate {
                card_id,
                title,
                description,
            } => {
                match Board::update_card_in(
                    &mut transaction,
                    &card_id,
                    &title,
                    description.as_deref(),
                )
                .await
                {
                    Ok(card) => AgentCommandResponse {
                        result: AgentCommandResult::Card { card },
                        effects: vec![AgentCommandEffect::CardUpdated { card_id }],
                    },
                    Err(error) => board_failure(error)?,
                }
            }
            AgentCommand::CardMove {
                card_id,
                column_id,
                before_card_id,
            } => {
                match Board::move_card_in(
                    &mut transaction,
                    &card_id,
                    &column_id,
                    before_card_id.as_deref(),
                )
                .await
                {
                    Ok(card) => AgentCommandResponse {
                        result: AgentCommandResult::Card { card: card.clone() },
                        effects: vec![AgentCommandEffect::CardMoved {
                            card_id,
                            column_id,
                            position: card.position,
                        }],
                    },
                    Err(error) => board_failure(error)?,
                }
            }
        };
        if mutating {
            save_result(
                &mut transaction,
                run_id
                    .as_deref()
                    .expect("every mutating AgentCommand requires an active Run"),
                &request.request_id,
                &response,
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(response)
    }

    async fn glance(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        run_id: &str,
        claims: &AgentClaims,
        room_id: &str,
    ) -> Result<AgentCommandResponse, sqlx::Error> {
        let compose_anchor: Option<i64> = sqlx::query_scalar(
            "SELECT COALESCE(delivery.up_to_seq, run.agenda_anchor_seq)
             FROM collab_runs run
             LEFT JOIN collab_run_deliveries delivery
               ON delivery.run_id = run.id AND delivery.room_id = $2
             JOIN collab_room_members member
               ON member.room_id = COALESCE(delivery.room_id, run.room_id)
              AND member.participant_id = $3
             WHERE run.id = $1 AND COALESCE(delivery.room_id, run.room_id) = $2
               AND run.status = 'running'
               AND (delivery.room_id IS NOT NULL OR run.trigger = 'agenda')",
        )
        .bind(run_id)
        .bind(room_id)
        .bind(&claims.sub)
        .fetch_optional(&mut **transaction)
        .await?;
        let Some(compose_anchor) = compose_anchor else {
            return Ok(error("NOT_FOUND", "Room is not in the active Run"));
        };
        let messages = sqlx::query_as::<_, MessageRow>(
            "SELECT id, room_id, sequence, author_id, body FROM (
                SELECT id, room_id, sequence, author_id, body
                FROM collab_messages
                WHERE room_id = $1 AND sequence > $2 AND author_id <> $3
                ORDER BY sequence DESC LIMIT 50
             ) recent ORDER BY sequence",
        )
        .bind(room_id)
        .bind(compose_anchor)
        .bind(&claims.sub)
        .fetch_all(&mut **transaction)
        .await?
        .into_iter()
        .map(MessageView::from)
        .collect::<Vec<_>>();
        let members = sqlx::query_as::<_, ParticipantRow>(
            "SELECT participant.id, participant.kind, participant.display_name
             FROM collab_room_members member
             JOIN collab_participants participant ON participant.id = member.participant_id
             WHERE member.room_id = $1
             ORDER BY participant.kind, participant.display_name, participant.id",
        )
        .bind(room_id)
        .fetch_all(&mut **transaction)
        .await?
        .into_iter()
        .map(ParticipantView::from)
        .collect::<Vec<_>>();
        if let Some(peer_max) = messages.last().map(|message| message.sequence)
            && let Err(error) = self
                .coordination
                .record_seen(&claims.sub, room_id, peer_max)
                .await
        {
            tracing::warn!(%error, %room_id, "glance seen update failed open");
        }
        Ok(success(AgentCommandResult::Glance {
            room_id: room_id.to_string(),
            compose_anchor,
            members,
            messages,
        }))
    }

    async fn reply(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        run_id: &str,
        claims: &AgentClaims,
        room_id: &str,
        body: &str,
        held_token: Option<&str>,
    ) -> Result<AgentCommandResponse, sqlx::Error> {
        if !valid_message_body(body) {
            return Ok(error("INVALID_ARGUMENT", "message body is invalid"));
        }
        let row: Option<(i64, String)> = sqlx::query_as(
            "SELECT COALESCE(delivery.up_to_seq, run.agenda_anchor_seq), room.kind
             FROM collab_runs run
             LEFT JOIN collab_run_deliveries delivery
               ON delivery.run_id = run.id AND delivery.room_id = $2
             JOIN collab_rooms room ON room.id = COALESCE(delivery.room_id, run.room_id)
             JOIN collab_room_members own
               ON own.room_id = room.id AND own.participant_id = $3
             WHERE run.id = $1 AND room.id = $2 AND run.status = 'running'
               AND (delivery.room_id IS NOT NULL OR run.trigger = 'agenda')
             FOR UPDATE OF room",
        )
        .bind(run_id)
        .bind(room_id)
        .bind(&claims.sub)
        .fetch_optional(&mut **transaction)
        .await?;
        let Some((snapshot_anchor, room_kind)) = row else {
            return Ok(error("NOT_FOUND", "Room is not in the active Run"));
        };
        let member_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM collab_room_members WHERE room_id = $1")
                .bind(room_id)
                .fetch_one(&mut **transaction)
                .await?;
        if room_kind == "direct" && held_token.is_some() {
            return Ok(error(
                "INVALID_ARGUMENT",
                "HELD token is not valid for a Direct Room",
            ));
        }
        if room_kind == "group" && member_count > 2 {
            match held_token {
                Some(token) => {
                    let binding = match self
                        .coordination
                        .consume_held(&claims.sub, room_id, token)
                        .await
                    {
                        Ok(Some(binding)) => binding,
                        Ok(None) => return Ok(error("HELD", "retry token is invalid or expired")),
                        Err(redis_error) => {
                            tracing::warn!(%redis_error, %room_id, "HELD token consumption failed closed");
                            return Ok(error(
                                "RATE_LIMITED",
                                "coordination is temporarily unavailable",
                            ));
                        }
                    };
                    if binding.agent_id != claims.sub
                        || binding.run_id != run_id
                        || binding.room_id != room_id
                        || binding.runtime_session_id != claims.runtime_session_id
                    {
                        return Ok(error("HELD", "retry token does not match this Run"));
                    }
                    if let Some(peer_max) =
                        peer_max(transaction, room_id, &claims.sub, binding.shown_peer_max).await?
                    {
                        return self
                            .hold_reply(
                                transaction,
                                run_id,
                                claims,
                                room_id,
                                binding.shown_peer_max,
                                peer_max,
                            )
                            .await;
                    }
                }
                None => {
                    let seen_baseline = match self.coordination.get_seen(&claims.sub, room_id).await
                    {
                        Ok(Some(sequence)) if sequence > 0 => sequence,
                        Ok(_) => snapshot_anchor,
                        Err(error) => {
                            tracing::warn!(%error, %room_id, "seen lookup failed; using durable snapshot");
                            snapshot_anchor
                        }
                    };
                    if let Some(peer_max) =
                        peer_max(transaction, room_id, &claims.sub, seen_baseline).await?
                    {
                        return self
                            .hold_reply(
                                transaction,
                                run_id,
                                claims,
                                room_id,
                                seen_baseline,
                                peer_max,
                            )
                            .await;
                    }
                }
            }
        }
        let message = insert_message(transaction, room_id, &claims.sub, "normal", body).await?;
        mark_delivery_action(transaction, run_id, room_id).await?;
        Ok(AgentCommandResponse {
            result: AgentCommandResult::MessagePublished {
                message: message.clone(),
            },
            effects: vec![message_effect(&message)],
        })
    }

    async fn hold_reply(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        run_id: &str,
        claims: &AgentClaims,
        room_id: &str,
        seen_baseline: i64,
        peer_max: i64,
    ) -> Result<AgentCommandResponse, sqlx::Error> {
        let messages = sqlx::query_as::<_, MessageRow>(
            "SELECT id, room_id, sequence, author_id, body
             FROM collab_messages
             WHERE room_id = $1 AND sequence > $2 AND sequence <= $3
               AND author_id <> $4
             ORDER BY sequence LIMIT 50",
        )
        .bind(room_id)
        .bind(seen_baseline)
        .bind(peer_max)
        .bind(&claims.sub)
        .fetch_all(&mut **transaction)
        .await?
        .into_iter()
        .map(MessageView::from)
        .collect::<Vec<_>>();
        let shown_peer_max = messages
            .last()
            .map(|message| message.sequence)
            .ok_or(sqlx::Error::RowNotFound)?;
        let binding = HeldBinding {
            agent_id: claims.sub.clone(),
            run_id: run_id.to_string(),
            room_id: room_id.to_string(),
            runtime_session_id: claims.runtime_session_id.clone(),
            shown_peer_max,
        };
        let retry_token = match self.coordination.issue_held(&binding).await {
            Ok(token) => token,
            Err(redis_error) => {
                tracing::warn!(%redis_error, %room_id, "HELD token issuance failed closed");
                return Ok(error(
                    "RATE_LIMITED",
                    "coordination is temporarily unavailable",
                ));
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

async fn active_run(
    transaction: &mut Transaction<'_, Postgres>,
    claims: &AgentClaims,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT run.id
         FROM collab_runs run
         JOIN collab_agent_profiles profile ON profile.agent_id = run.agent_id
         WHERE run.agent_id = $1 AND run.runtime_session_id = $2
           AND run.status = 'running' AND profile.archived_at IS NULL
         FOR UPDATE OF run",
    )
    .bind(&claims.sub)
    .bind(&claims.runtime_session_id)
    .fetch_optional(&mut **transaction)
    .await
}

async fn current_inbox(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
    claims: &AgentClaims,
) -> Result<AgentCommandResponse, sqlx::Error> {
    let carried_over: bool = sqlx::query_scalar(
        "SELECT inbox_carried_over FROM collab_runs
         WHERE id = $1 AND agent_id = $2 AND runtime_session_id = $3
           AND status = 'running'",
    )
    .bind(run_id)
    .bind(&claims.sub)
    .bind(&claims.runtime_session_id)
    .fetch_one(&mut **transaction)
    .await?;
    let messages = sqlx::query_as::<_, MessageRow>(
        "SELECT message.id, message.room_id, message.sequence,
                message.author_id, message.body
         FROM collab_run_deliveries delivery
         JOIN collab_runs run ON run.id = delivery.run_id
         JOIN collab_messages message ON message.room_id = delivery.room_id
           AND message.sequence BETWEEN delivery.from_seq AND delivery.up_to_seq
         WHERE delivery.run_id = $1 AND run.agent_id = $2
           AND run.runtime_session_id = $3 AND run.status = 'running'
           AND message.author_id <> $2
         ORDER BY delivery.room_id, message.sequence",
    )
    .bind(run_id)
    .bind(&claims.sub)
    .bind(&claims.runtime_session_id)
    .fetch_all(&mut **transaction)
    .await?
    .into_iter()
    .map(MessageView::from)
    .collect();
    Ok(success(AgentCommandResult::Inbox {
        carried_over,
        messages,
    }))
}

async fn ack(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
    room_id: &str,
) -> Result<AgentCommandResponse, sqlx::Error> {
    let up_to_seq: Option<i64> = sqlx::query_scalar(
        "UPDATE collab_run_deliveries
         SET eligible_reason = CASE WHEN eligible_reason = 'action' THEN 'action' ELSE 'ack' END,
             eligible_at = COALESCE(
                 eligible_at, CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             )
         WHERE run_id = $1 AND room_id = $2 RETURNING up_to_seq",
    )
    .bind(run_id)
    .bind(room_id)
    .fetch_optional(&mut **transaction)
    .await?;
    let Some(up_to_seq) = up_to_seq else {
        return Ok(error("NOT_FOUND", "Room is not in the active Run"));
    };
    Ok(AgentCommandResponse {
        result: AgentCommandResult::Acknowledged {
            room_id: room_id.to_string(),
            up_to_seq,
        },
        effects: vec![AgentCommandEffect::InboxAcknowledged {
            room_id: room_id.to_string(),
            up_to_seq,
        }],
    })
}

async fn direct_message(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
    claims: &AgentClaims,
    participant_id: &str,
    body: &str,
) -> Result<AgentCommandResponse, sqlx::Error> {
    if participant_id == claims.sub {
        return Ok(error("INVALID_ARGUMENT", "cannot DM yourself"));
    }
    if !valid_message_body(body) {
        return Ok(error("INVALID_ARGUMENT", "message body is invalid"));
    }
    let participant_active: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM collab_participants participant
            LEFT JOIN collab_agent_profiles profile ON profile.agent_id = participant.id
            WHERE participant.id = $1
              AND (participant.kind = 'user' OR profile.archived_at IS NULL)
         )",
    )
    .bind(participant_id)
    .fetch_one(&mut **transaction)
    .await?;
    if !participant_active {
        return Ok(error(
            "NOT_FOUND",
            "participant does not exist or is archived",
        ));
    }
    let (room_id, _) =
        get_or_create_direct_room(transaction, &claims.sub, participant_id, &claims.sub).await?;
    let message = insert_message(transaction, &room_id, &claims.sub, "normal", body).await?;
    mark_delivery_action(transaction, run_id, &room_id).await?;
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

async fn insert_message(
    transaction: &mut Transaction<'_, Postgres>,
    room_id: &str,
    author_id: &str,
    kind: &str,
    body: &str,
) -> Result<MessageView, sqlx::Error> {
    let sequence: i64 = sqlx::query_scalar(
        "UPDATE collab_rooms
         SET next_seq = next_seq + 1,
             last_message_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
             updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
         WHERE id = $1 RETURNING next_seq",
    )
    .bind(room_id)
    .fetch_one(&mut **transaction)
    .await?;
    let id = entity_id("msg");
    sqlx::query(
        "INSERT INTO collab_messages (id, room_id, sequence, author_id, kind, body)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(&id)
    .bind(room_id)
    .bind(sequence)
    .bind(author_id)
    .bind(kind)
    .bind(body)
    .execute(&mut **transaction)
    .await?;
    Ok(MessageView {
        id,
        room_id: room_id.to_string(),
        sequence,
        author_id: author_id.to_string(),
        body: body.to_string(),
    })
}

async fn mark_delivery_action(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
    room_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE collab_run_deliveries
         SET eligible_reason = 'action',
             eligible_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
         WHERE run_id = $1 AND room_id = $2",
    )
    .bind(run_id)
    .bind(room_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn peer_max(
    transaction: &mut Transaction<'_, Postgres>,
    room_id: &str,
    agent_id: &str,
    seen_baseline: i64,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT MAX(sequence) FROM collab_messages
         WHERE room_id = $1 AND sequence > $2 AND author_id <> $3",
    )
    .bind(room_id)
    .bind(seen_baseline)
    .bind(agent_id)
    .fetch_one(&mut **transaction)
    .await
}

async fn reserve_request(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
    claims: &AgentClaims,
    request_id: &str,
    semantic_hash: &str,
) -> Result<Option<AgentCommandResponse>, sqlx::Error> {
    let inserted: Option<String> = sqlx::query_scalar(
        "INSERT INTO collab_command_requests (
            id, runtime_session_id, run_id, request_id, actor_id, semantic_hash
         ) VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT DO NOTHING RETURNING id",
    )
    .bind(entity_id("cmd"))
    .bind(&claims.runtime_session_id)
    .bind(run_id)
    .bind(request_id)
    .bind(&claims.sub)
    .bind(semantic_hash)
    .fetch_optional(&mut **transaction)
    .await?;
    if inserted.is_some() {
        return Ok(None);
    }
    let existing: Option<(String, Option<serde_json::Value>)> = sqlx::query_as(
        "SELECT semantic_hash, result FROM collab_command_requests
         WHERE run_id = $1 AND request_id = $2 FOR UPDATE",
    )
    .bind(run_id)
    .bind(request_id)
    .fetch_optional(&mut **transaction)
    .await?;
    let Some((existing_hash, result)) = existing else {
        return Err(sqlx::Error::Protocol(
            "CONFLICT: command request collision".to_string(),
        ));
    };
    if existing_hash != semantic_hash {
        return Ok(Some(error(
            "CONFLICT",
            "request id was already used for a different command",
        )));
    }
    let Some(result) = result else {
        return Ok(Some(error("CONFLICT", "command request is still running")));
    };
    serde_json::from_value(result)
        .map(Some)
        .map_err(|error| sqlx::Error::Protocol(format!("invalid stored command result: {error}")))
}

async fn save_result(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: &str,
    request_id: &str,
    result: &AgentCommandResponse,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE collab_command_requests SET result = $3
         WHERE run_id = $1 AND request_id = $2",
    )
    .bind(run_id)
    .bind(request_id)
    .bind(serde_json::to_value(result).expect("AgentCommandResponse is serializable"))
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn valid_request_id(request_id: &str) -> bool {
    request_id.len() == 36
        && request_id.starts_with("req-")
        && request_id[4..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn valid_message_body(body: &str) -> bool {
    !body.trim().is_empty()
        && body.len() <= crate::protocol::MESSAGE_BODY_MAX_BYTES
        && !body.as_bytes().contains(&0)
}

fn digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(7 + digest.len() * 2);
    encoded.push_str("sha256:");
    for byte in digest {
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

fn success(result: AgentCommandResult) -> AgentCommandResponse {
    AgentCommandResponse {
        result,
        effects: Vec::new(),
    }
}

fn error(code: &str, message: &str) -> AgentCommandResponse {
    success(AgentCommandResult::Error {
        code: code.to_string(),
        message: message.to_string(),
    })
}

fn message_effect(message: &MessageView) -> AgentCommandEffect {
    AgentCommandEffect::MessagePublished {
        room_id: message.room_id.clone(),
        message_id: message.id.clone(),
        sequence: message.sequence,
    }
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

#[derive(FromRow)]
struct ParticipantRow {
    id: String,
    kind: String,
    display_name: String,
}

impl From<ParticipantRow> for ParticipantView {
    fn from(row: ParticipantRow) -> Self {
        Self {
            id: row.id,
            kind: row.kind,
            display_name: row.display_name,
        }
    }
}

fn board_failure(failure: BoardOperationError) -> Result<AgentCommandResponse, sqlx::Error> {
    match failure {
        BoardOperationError::Domain { code, message } => Ok(error(code, message)),
        BoardOperationError::Database(error) => Err(error),
    }
}

fn climate_failure(failure: ClimateOperationError) -> Result<AgentCommandResponse, sqlx::Error> {
    match failure {
        ClimateOperationError::Domain { code, message } => Ok(error(code, message)),
        ClimateOperationError::Database(error) => Err(error),
    }
}
