use sqlx::{PgPool, Postgres, Transaction};

use crate::protocol::{
    AgentCommand, AgentCommandEffect, AgentCommandRequest, AgentCommandResponse,
    AgentCommandResult, MessageView,
};

use super::{
    agents::Agents,
    auth::{AgentClaims, authorize_agent_transaction},
    board::{Board, BoardOperationError},
    climate::{Climate, ClimateOperationError},
    command_requests::CommandRequests,
    coordination::{Coordination, HeldBinding},
    messages::Messages,
    rooms::{Rooms, get_or_create_direct_room},
    runs::Runs,
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
        if !CommandRequests::valid_id(&request.request_id) {
            return Ok(error("INVALID_ARGUMENT", "invalid request id"));
        }
        let mut transaction = self.pool.begin().await?;
        authorize_agent_transaction(&mut transaction, claims).await?;
        let mutating = request.command.is_mutating();
        let run_id = if request.command.requires_active_run() {
            let Some(run_id) = Runs::active_in(&mut transaction, claims).await? else {
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
            let semantic_hash = CommandRequests::semantic_hash(
                &serde_json::to_vec(&request.command).expect("AgentCommand is serializable"),
            );
            if let Some(result) = CommandRequests::reserve_in(
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
                let (carried_over, messages) = Runs::inbox_in(
                    &mut transaction,
                    run_id.as_deref().expect("Inbox requires an active Run"),
                    claims,
                )
                .await?;
                success(AgentCommandResult::Inbox {
                    carried_over,
                    messages,
                })
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
                let up_to_seq = Runs::acknowledge_in(
                    &mut transaction,
                    run_id.as_deref().expect("Ack requires an active Run"),
                    &room_id,
                )
                .await?;
                match up_to_seq {
                    Some(up_to_seq) => AgentCommandResponse {
                        result: AgentCommandResult::Acknowledged {
                            room_id: room_id.clone(),
                            up_to_seq,
                        },
                        effects: vec![AgentCommandEffect::InboxAcknowledged { room_id, up_to_seq }],
                    },
                    None => error("NOT_FOUND", "Room is not in the active Run"),
                }
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
            CommandRequests::save_in(
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
        let compose_anchor =
            Runs::glance_anchor_in(transaction, run_id, room_id, &claims.sub).await?;
        let Some(compose_anchor) = compose_anchor else {
            return Ok(error("NOT_FOUND", "Room is not in the active Run"));
        };
        let messages =
            Messages::glance_in(transaction, room_id, compose_anchor, &claims.sub).await?;
        let members = Rooms::list_members_for_agent_in(transaction, &claims.sub, room_id).await?;
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
        if !Messages::valid_body(body) {
            return Ok(error("INVALID_ARGUMENT", "message body is invalid"));
        }
        let context = Runs::reply_context_in(transaction, run_id, room_id, &claims.sub).await?;
        let Some((snapshot_anchor, room_kind, member_count)) = context else {
            return Ok(error("NOT_FOUND", "Room is not in the active Run"));
        };
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
                    if let Some(peer_max) = Messages::peer_max_in(
                        transaction,
                        room_id,
                        &claims.sub,
                        binding.shown_peer_max,
                    )
                    .await?
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
                        Messages::peer_max_in(transaction, room_id, &claims.sub, seen_baseline)
                            .await?
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
        let message = Messages::insert_agent_in(transaction, room_id, &claims.sub, body).await?;
        Runs::mark_delivery_action_in(transaction, run_id, room_id).await?;
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
        let messages =
            Messages::between_in(transaction, room_id, seen_baseline, peer_max, &claims.sub)
                .await?;
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
    let message = Messages::insert_agent_in(transaction, &room_id, &claims.sub, body).await?;
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
