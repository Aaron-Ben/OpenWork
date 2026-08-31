use std::fmt::Write as _;

use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};

use crate::protocol::{
    DesktopCommand, DesktopCommandRequest, DesktopCommandResult, RuntimeStatusView, entity_id,
};

use super::{
    agents::Agents,
    board::{Board, BoardOperationError},
    inventory::EngineInventory,
    messages::Messages,
    rooms::Rooms,
    runs::Runs,
    runtime_session::RuntimeSession,
    scheduler::Scheduler,
};

#[derive(Clone)]
pub(crate) struct DesktopCommands {
    pool: PgPool,
    agents: Agents,
    board: Board,
    inventory: EngineInventory,
    messages: Messages,
    rooms: Rooms,
    runs: Runs,
    scheduler: Scheduler,
    session: RuntimeSession,
}

enum PostCommitEffect {
    AgentConfig {
        agent_id: String,
        revision: i64,
    },
    MessageCommitted {
        message_id: String,
        room_id: String,
        author_id: String,
    },
}

impl DesktopCommands {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        pool: PgPool,
        agents: Agents,
        board: Board,
        inventory: EngineInventory,
        messages: Messages,
        rooms: Rooms,
        runs: Runs,
        scheduler: Scheduler,
        session: RuntimeSession,
    ) -> Self {
        Self {
            pool,
            agents,
            board,
            inventory,
            messages,
            rooms,
            runs,
            scheduler,
            session,
        }
    }

    pub(crate) async fn execute(
        &self,
        request: DesktopCommandRequest,
    ) -> Result<DesktopCommandResult, sqlx::Error> {
        if !request.command.is_mutating() {
            return self.dispatch_read(request.command).await;
        }
        let request_id = request.request_id.as_deref().ok_or_else(|| {
            protocol_error("INVALID_ARGUMENT: mutating Desktop command needs requestId")
        })?;
        if !valid_request_id(request_id) {
            return Err(protocol_error("INVALID_ARGUMENT: invalid request id"));
        }
        let semantic_hash =
            digest(&serde_json::to_vec(&request.command).expect("DesktopCommand is serializable"));
        let mut transaction = self.pool.begin().await?;
        if let Some(result) = self
            .reserve_in(&mut transaction, request_id, &semantic_hash)
            .await?
        {
            transaction.commit().await?;
            return Ok(result);
        }
        let (result, effects) = self
            .dispatch_mutating_in(&mut transaction, request.command)
            .await?;
        self.save_in(&mut transaction, request_id, &result).await?;
        transaction.commit().await?;
        self.apply_effects(effects).await;
        Ok(result)
    }

    async fn dispatch_read(
        &self,
        command: DesktopCommand,
    ) -> Result<DesktopCommandResult, sqlx::Error> {
        match command {
            DesktopCommand::Status => Ok(DesktopCommandResult::Status(RuntimeStatusView {
                runtime_session_id: self.session.id().to_string(),
                started_at: self.session.started_at(),
                last_computer_heartbeat: self.session.last_computer_heartbeat(),
                engines: self.inventory.list().await?,
                engine_readiness: self.session.engine_readiness(),
                runners: self.session.runner_statuses(),
            })),
            DesktopCommand::ListAgents => Ok(DesktopCommandResult::Agents {
                agents: self.agents.list().await?,
            }),
            DesktopCommand::ListRooms => Ok(DesktopCommandResult::Rooms {
                rooms: self.rooms.list().await?,
            }),
            DesktopCommand::ListRoomMembers { room_id } => Ok(DesktopCommandResult::Members {
                members: self.rooms.list_members(&room_id).await?,
            }),
            DesktopCommand::ListMessages { room_id } => Ok(DesktopCommandResult::Messages {
                messages: self.messages.list(&room_id).await?,
            }),
            DesktopCommand::ListBoards => Ok(DesktopCommandResult::Boards {
                boards: self.board.list().await?,
            }),
            DesktopCommand::ListRuns { limit } => Ok(DesktopCommandResult::Runs {
                runs: self.runs.list(limit).await?,
            }),
            _ => Err(protocol_error(
                "INVALID_ARGUMENT: mutating Desktop command reached read dispatcher",
            )),
        }
    }

    async fn dispatch_mutating_in(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        command: DesktopCommand,
    ) -> Result<(DesktopCommandResult, Vec<PostCommitEffect>), sqlx::Error> {
        let (result, effects) = match command {
            DesktopCommand::CreateAgent {
                display_name,
                role,
                persona,
                engine_id,
                main_model_id,
                triage_model_id,
            } => {
                let agent = Agents::create_in(
                    transaction,
                    &display_name,
                    role.as_deref(),
                    &persona,
                    &engine_id,
                    &main_model_id,
                    &triage_model_id,
                )
                .await?;
                let effect = agent_effect(&agent);
                (DesktopCommandResult::Agent(agent), vec![effect])
            }
            DesktopCommand::SetAgentAgenda { agent_id, enabled } => {
                let agent = Agents::set_agenda_in(transaction, &agent_id, enabled).await?;
                let effect = agent_effect(&agent);
                (DesktopCommandResult::Agent(agent), vec![effect])
            }
            DesktopCommand::UpdateAgent {
                agent_id,
                display_name,
                role,
                persona,
                engine_id,
                main_model_id,
                triage_model_id,
            } => {
                let agent = Agents::update_in(
                    transaction,
                    &agent_id,
                    &display_name,
                    role.as_deref(),
                    &persona,
                    &engine_id,
                    &main_model_id,
                    &triage_model_id,
                )
                .await?;
                let effect = agent_effect(&agent);
                (DesktopCommandResult::Agent(agent), vec![effect])
            }
            DesktopCommand::ArchiveAgent { agent_id } => {
                let agent = Agents::set_archived_in(transaction, &agent_id, true).await?;
                let effect = agent_effect(&agent);
                (DesktopCommandResult::Agent(agent), vec![effect])
            }
            DesktopCommand::RestoreAgent { agent_id } => {
                let agent = Agents::set_archived_in(transaction, &agent_id, false).await?;
                let effect = agent_effect(&agent);
                (DesktopCommandResult::Agent(agent), vec![effect])
            }
            DesktopCommand::CreateDirectRoom { agent_id } => (
                DesktopCommandResult::Room(Rooms::create_direct_in(transaction, &agent_id).await?),
                Vec::new(),
            ),
            DesktopCommand::CreateGroupRoom { title, agent_ids } => (
                DesktopCommandResult::Room(
                    Rooms::create_group_in(transaction, &title, &agent_ids).await?,
                ),
                Vec::new(),
            ),
            DesktopCommand::AddGroupMember { room_id, agent_id } => {
                let (members, message) =
                    Rooms::change_member_in(transaction, &room_id, &agent_id, true).await?;
                let effects = message.into_iter().map(message_effect).collect();
                (DesktopCommandResult::Members { members }, effects)
            }
            DesktopCommand::RemoveGroupMember { room_id, agent_id } => {
                let (members, message) =
                    Rooms::change_member_in(transaction, &room_id, &agent_id, false).await?;
                let effects = message.into_iter().map(message_effect).collect();
                (DesktopCommandResult::Members { members }, effects)
            }
            DesktopCommand::SendMessage { room_id, body } => {
                let message = Messages::send_user_in(transaction, &room_id, &body).await?;
                let effect = message_effect(message.clone());
                (DesktopCommandResult::Message(message), vec![effect])
            }
            DesktopCommand::CreateBoard { title, description } => (
                DesktopCommandResult::Board(
                    Board::create_in(transaction, &title, description.as_deref(), "local-user")
                        .await?,
                ),
                Vec::new(),
            ),
            DesktopCommand::UpdateBoard {
                board_id,
                title,
                description,
            } => (
                DesktopCommandResult::Board(
                    Board::update_in(transaction, &board_id, &title, description.as_deref())
                        .await
                        .map_err(board_error)?,
                ),
                Vec::new(),
            ),
            DesktopCommand::DeleteBoard { board_id } => {
                Board::delete_in(transaction, &board_id)
                    .await
                    .map_err(board_error)?;
                (
                    DesktopCommandResult::Deleted {
                        entity_id: board_id,
                    },
                    Vec::new(),
                )
            }
            DesktopCommand::CreateBoardColumn {
                board_id,
                title,
                is_terminal,
            } => (
                DesktopCommandResult::Board(
                    Board::create_column_in(transaction, &board_id, &title, is_terminal)
                        .await
                        .map_err(board_error)?,
                ),
                Vec::new(),
            ),
            DesktopCommand::UpdateBoardColumn {
                column_id,
                title,
                is_terminal,
            } => (
                DesktopCommandResult::Board(
                    Board::update_column_in(transaction, &column_id, &title, is_terminal)
                        .await
                        .map_err(board_error)?,
                ),
                Vec::new(),
            ),
            DesktopCommand::MoveBoardColumn {
                column_id,
                before_column_id,
            } => (
                DesktopCommandResult::Board(
                    Board::move_column_in(transaction, &column_id, before_column_id.as_deref())
                        .await
                        .map_err(board_error)?,
                ),
                Vec::new(),
            ),
            DesktopCommand::DeleteBoardColumn { column_id } => (
                DesktopCommandResult::Board(
                    Board::delete_column_in(transaction, &column_id)
                        .await
                        .map_err(board_error)?,
                ),
                Vec::new(),
            ),
            DesktopCommand::AssignCard {
                card_id,
                assignee_id,
            } => (
                DesktopCommandResult::Card(
                    Board::assign_card_in(transaction, &card_id, assignee_id.as_deref())
                        .await
                        .map_err(board_error)?,
                ),
                Vec::new(),
            ),
            DesktopCommand::DeleteCard { card_id } => {
                Board::delete_card_in(transaction, &card_id)
                    .await
                    .map_err(board_error)?;
                (
                    DesktopCommandResult::Deleted { entity_id: card_id },
                    Vec::new(),
                )
            }
            _ => {
                return Err(protocol_error(
                    "INVALID_ARGUMENT: read-only Desktop command reached mutation dispatcher",
                ));
            }
        };
        Ok((result, effects))
    }

    async fn apply_effects(&self, effects: Vec<PostCommitEffect>) {
        for effect in effects {
            match effect {
                PostCommitEffect::AgentConfig { agent_id, revision } => {
                    self.session.publish_agent_config(&agent_id, revision);
                }
                PostCommitEffect::MessageCommitted {
                    message_id,
                    room_id,
                    author_id,
                } => {
                    self.scheduler
                        .message_committed(&message_id, &room_id, &author_id)
                        .await;
                }
            }
        }
    }

    async fn reserve_in(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        request_id: &str,
        semantic_hash: &str,
    ) -> Result<Option<DesktopCommandResult>, sqlx::Error> {
        let inserted: Option<String> = sqlx::query_scalar(
            "INSERT INTO collab_command_requests (
                id, runtime_session_id, run_id, request_id, actor_id, semantic_hash
             ) VALUES ($1, $2, NULL, $3, 'local-user', $4)
             ON CONFLICT DO NOTHING RETURNING id",
        )
        .bind(entity_id("cmd"))
        .bind(self.session.id())
        .bind(request_id)
        .bind(semantic_hash)
        .fetch_optional(&mut **transaction)
        .await?;
        if inserted.is_some() {
            return Ok(None);
        }
        let existing: Option<(String, Option<serde_json::Value>)> = sqlx::query_as(
            "SELECT semantic_hash, result FROM collab_command_requests
             WHERE runtime_session_id = $1 AND run_id IS NULL AND request_id = $2
             FOR UPDATE",
        )
        .bind(self.session.id())
        .bind(request_id)
        .fetch_optional(&mut **transaction)
        .await?;
        let Some((existing_hash, result)) = existing else {
            return Err(protocol_error(
                "CONFLICT: Desktop request reservation disappeared",
            ));
        };
        if existing_hash != semantic_hash {
            return Err(protocol_error(
                "CONFLICT: request id was already used for a different command",
            ));
        }
        let result = result
            .ok_or_else(|| protocol_error("CONFLICT: previous Desktop command did not finish"))?;
        serde_json::from_value(result)
            .map(Some)
            .map_err(|error| protocol_error(&format!("PROTOCOL: invalid stored result: {error}")))
    }

    async fn save_in(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        request_id: &str,
        result: &DesktopCommandResult,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE collab_command_requests SET result = $3
             WHERE runtime_session_id = $1 AND run_id IS NULL AND request_id = $2",
        )
        .bind(self.session.id())
        .bind(request_id)
        .bind(serde_json::to_value(result).expect("DesktopCommandResult is serializable"))
        .execute(&mut **transaction)
        .await?;
        Ok(())
    }
}

fn agent_effect(agent: &crate::protocol::AgentView) -> PostCommitEffect {
    PostCommitEffect::AgentConfig {
        agent_id: agent.id.clone(),
        revision: agent.config_revision,
    }
}

fn message_effect(message: crate::protocol::MessageView) -> PostCommitEffect {
    PostCommitEffect::MessageCommitted {
        message_id: message.id,
        room_id: message.room_id,
        author_id: message.author_id,
    }
}

fn board_error(error: BoardOperationError) -> sqlx::Error {
    match error {
        BoardOperationError::Domain { code, message } => {
            protocol_error(&format!("{code}: {message}"))
        }
        BoardOperationError::Database(error) => error,
    }
}

fn valid_request_id(request_id: &str) -> bool {
    request_id.len() == 36
        && request_id.starts_with("req-")
        && request_id[4..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
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

fn protocol_error(message: &str) -> sqlx::Error {
    sqlx::Error::Protocol(message.to_string())
}
