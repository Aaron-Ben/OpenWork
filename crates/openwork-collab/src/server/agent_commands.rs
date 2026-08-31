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
    coordination::{Coordination, HeldBinding, HeldReservation},
    messages::Messages,
    rooms::{Rooms, get_or_create_direct_room},
    runs::Runs,
    triage::AGENT_LOOP_HARD_CAP,
};

#[derive(Clone)]
pub(crate) struct AgentCommands {
    pool: PgPool,
    coordination: Coordination,
}

struct ReplyInput<'a> {
    room_id: &'a str,
    body: &'a str,
    held_token: Option<&'a str>,
    request_id: &'a str,
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
        let request_id = request.request_id.clone();
        let held_target = match &request.command {
            AgentCommand::Reply {
                room_id,
                held_token: Some(token),
                ..
            } => Some((room_id.clone(), token.clone())),
            _ => None,
        };
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
                &request_id,
                &semantic_hash,
            )
            .await?
            {
                transaction.commit().await?;
                if let Some((room_id, token)) = held_target.as_ref() {
                    self.finalize_held(claims, room_id, token, &request_id)
                        .await;
                }
                return Ok(result);
            }
        }

        let mut held_reserved = false;
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
                let (response, reserved) = self
                    .reply(
                        &mut transaction,
                        run_id.as_deref().expect("Reply requires an active Run"),
                        claims,
                        ReplyInput {
                            room_id: &room_id,
                            body: &body,
                            held_token: held_token.as_deref(),
                            request_id: &request_id,
                        },
                    )
                    .await?;
                held_reserved = reserved;
                response
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
                &request_id,
                &response,
            )
            .await?;
        }
        transaction.commit().await?;
        if held_reserved && let Some((room_id, token)) = held_target.as_ref() {
            self.finalize_held(claims, room_id, token, &request_id)
                .await;
        }
        Ok(response)
    }

    async fn finalize_held(
        &self,
        claims: &AgentClaims,
        room_id: &str,
        token: &str,
        request_id: &str,
    ) {
        match self
            .coordination
            .finalize_held(&claims.sub, room_id, token, request_id)
            .await
        {
            Ok(true) => {}
            Ok(false) => {
                tracing::warn!(%room_id, %request_id, "HELD reservation was not finalized")
            }
            Err(error) => {
                tracing::warn!(%error, %room_id, %request_id, "HELD finalization failed; the same request may retry")
            }
        }
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
        input: ReplyInput<'_>,
    ) -> Result<(AgentCommandResponse, bool), sqlx::Error> {
        let ReplyInput {
            room_id,
            body,
            held_token,
            request_id,
        } = input;
        if !Messages::valid_body(body) {
            return Ok((error("INVALID_ARGUMENT", "message body is invalid"), false));
        }
        let context = Runs::reply_context_in(transaction, run_id, room_id, &claims.sub).await?;
        let Some((snapshot_anchor, room_kind, member_count)) = context else {
            return Ok((error("NOT_FOUND", "Room is not in the active Run"), false));
        };
        if room_kind == "direct" && held_token.is_some() {
            return Ok((
                error(
                    "INVALID_ARGUMENT",
                    "HELD token is not valid for a Direct Room",
                ),
                false,
            ));
        }
        if Messages::agent_loop_capped_in(transaction, room_id, AGENT_LOOP_HARD_CAP).await? {
            return Ok((
                error(
                    "LOOP_CAP",
                    "Agent conversation reached its deterministic loop cap",
                ),
                false,
            ));
        }
        let mut held_reserved = false;
        if room_kind == "group" && member_count > 2 {
            match held_token {
                Some(token) => {
                    let binding = match self
                        .coordination
                        .reserve_held(&claims.sub, room_id, token, request_id)
                        .await
                    {
                        Ok(HeldReservation::Reserved(binding)) => {
                            held_reserved = true;
                            binding
                        }
                        Ok(HeldReservation::Missing) => {
                            return Ok((error("HELD", "retry token is invalid or expired"), false));
                        }
                        Ok(HeldReservation::OwnedByAnotherRequest) => {
                            return Ok((
                                error("HELD", "retry token belongs to another request"),
                                false,
                            ));
                        }
                        Err(redis_error) => {
                            tracing::warn!(%redis_error, %room_id, "HELD token reservation failed closed");
                            return Ok((
                                error("RATE_LIMITED", "coordination is temporarily unavailable"),
                                false,
                            ));
                        }
                    };
                    if binding.agent_id != claims.sub
                        || binding.run_id != run_id
                        || binding.room_id != room_id
                        || binding.runtime_session_id != claims.runtime_session_id
                    {
                        return Ok((
                            error("HELD", "retry token does not match this Run"),
                            held_reserved,
                        ));
                    }
                    if let Some(peer_max) = Messages::peer_max_in(
                        transaction,
                        room_id,
                        &claims.sub,
                        binding.shown_peer_max,
                    )
                    .await?
                    {
                        let response = self
                            .hold_reply(
                                transaction,
                                run_id,
                                claims,
                                room_id,
                                binding.shown_peer_max,
                                peer_max,
                            )
                            .await?;
                        return Ok((response, held_reserved));
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
                        let response = self
                            .hold_reply(
                                transaction,
                                run_id,
                                claims,
                                room_id,
                                seen_baseline,
                                peer_max,
                            )
                            .await?;
                        return Ok((response, false));
                    }
                }
            }
        }
        let message = Messages::insert_agent_in(transaction, room_id, &claims.sub, body).await?;
        Runs::mark_delivery_action_in(transaction, run_id, room_id).await?;
        Ok((
            AgentCommandResponse {
                result: AgentCommandResult::MessagePublished {
                    message: message.clone(),
                },
                effects: vec![message_effect(&message)],
            },
            held_reserved,
        ))
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
    if Messages::agent_loop_capped_in(transaction, &room_id, AGENT_LOOP_HARD_CAP).await? {
        return Ok(error(
            "LOOP_CAP",
            "Agent conversation reached its deterministic loop cap",
        ));
    }
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
