//! Agent 的 Board 与 Card 命令（collaboration.md §11.2–§11.4）：读取、创建、领取、改派、更新与移动，
//! 以及创建、改派、更新产生的卡片唤醒。

use sqlx::{Postgres, Transaction};

use super::{AgentCommands, error, success};
use crate::protocol::{
    AgentCommand, AgentCommandEffect, AgentCommandResponse, AgentCommandResult, CardView,
};
use crate::server::{
    auth::AgentClaims,
    board::{Board, BoardOperationError, CardEdit, NewCard},
    card_wakes::CardWakes,
};

impl AgentCommands {
    /// 执行一条 Board 或 Card 命令。`execute` 只把这些变体分派到这里。
    pub(super) async fn card_command(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        claims: &AgentClaims,
        command: AgentCommand,
    ) -> Result<AgentCommandResponse, sqlx::Error> {
        let actor_id = claims.sub.as_str();
        match command {
            AgentCommand::CardCreate {
                board_id,
                column_id,
                title,
                description,
                assignee_id,
            } => {
                let new_card = NewCard {
                    board_id: &board_id,
                    column_id: &column_id,
                    title: &title,
                    description: description.as_deref(),
                    assignee_id: assignee_id.as_deref(),
                    actor_id,
                };
                match Board::create_card_in(transaction, new_card).await {
                    Ok(card) => {
                        let effect = AgentCommandEffect::CardCreated {
                            board_id,
                            card_id: card.id.clone(),
                        };
                        self.with_wakes(transaction, None, card, effect, actor_id)
                            .await
                    }
                    Err(error) => board_failure(error),
                }
            }
            AgentCommand::CardAssign {
                card_id,
                assignee_id,
            } => {
                let edit = Board::assign_card_in(transaction, &card_id, Some(&assignee_id)).await;
                let effect = AgentCommandEffect::CardAssigned {
                    card_id,
                    assignee_id,
                };
                self.edited(transaction, edit, effect, actor_id).await
            }
            AgentCommand::CardUpdate {
                card_id,
                title,
                description,
            } => {
                let edit =
                    Board::update_card_in(transaction, &card_id, &title, description.as_deref())
                        .await;
                let effect = AgentCommandEffect::CardUpdated { card_id };
                self.edited(transaction, edit, effect, actor_id).await
            }
            command => without_wakes(transaction, command, actor_id).await,
        }
    }

    async fn edited(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        edit: Result<CardEdit, BoardOperationError>,
        effect: AgentCommandEffect,
        actor_id: &str,
    ) -> Result<AgentCommandResponse, sqlx::Error> {
        match edit {
            Ok(CardEdit { before, after }) => {
                self.with_wakes(transaction, Some(&before), after, effect, actor_id)
                    .await
            }
            Err(error) => board_failure(error),
        }
    }

    /// 在同一事务里写入这次变化产生的卡片唤醒，返回卡片结果与全部 effect。Agent 触发的唤醒超过
    /// 接收者每分钟 30 次的限额时不写入；Redis 不可用时放行（collaboration.md §11.4）。
    async fn with_wakes(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        before: Option<&CardView>,
        card: CardView,
        effect: AgentCommandEffect,
        actor_id: &str,
    ) -> Result<AgentCommandResponse, sqlx::Error> {
        let mut effects = vec![effect];
        for (agent_id, reason) in
            CardWakes::targets_in(transaction, before, &card, actor_id).await?
        {
            if !self.within_wake_limit(&agent_id).await {
                continue;
            }
            CardWakes::record_in(transaction, &card.id, &agent_id, reason).await?;
            effects.push(AgentCommandEffect::CardWakeQueued {
                agent_id,
                card_id: card.id.clone(),
            });
        }
        Ok(AgentCommandResponse {
            result: AgentCommandResult::Card { card },
            effects,
        })
    }

    async fn within_wake_limit(&self, agent_id: &str) -> bool {
        match self.coordination.allow_agent_authored_wake(agent_id).await {
            Ok(allowed) => allowed,
            Err(error) => {
                tracing::warn!(%error, agent_id, "card wake rate check failed open");
                true
            }
        }
    }
}

/// 不产生卡片唤醒的 Board/Card 命令：读取、领取（负责人只会变成发起者本人）与移动。
async fn without_wakes(
    transaction: &mut Transaction<'_, Postgres>,
    command: AgentCommand,
    actor_id: &str,
) -> Result<AgentCommandResponse, sqlx::Error> {
    match command {
        AgentCommand::BoardList => Ok(success(AgentCommandResult::Boards {
            boards: Board::list_in(transaction).await?,
        })),
        AgentCommand::BoardShow { board_id } => match Board::get_in(transaction, &board_id).await {
            Ok(board) => Ok(success(AgentCommandResult::Board { board })),
            Err(sqlx::Error::RowNotFound) => Ok(error("NOT_FOUND", "Board does not exist")),
            Err(error) => Err(error),
        },
        AgentCommand::CardList { board_id } => Ok(success(AgentCommandResult::Cards {
            cards: Board::list_cards_in(transaction, board_id.as_deref()).await?,
        })),
        AgentCommand::CardShow { card_id } => match Board::get_card_in(transaction, &card_id).await
        {
            Ok(card) => Ok(success(AgentCommandResult::Card { card })),
            Err(error) => board_failure(error),
        },
        AgentCommand::CardClaim { card_id } => {
            let claimed = Board::claim_card_in(transaction, &card_id, actor_id).await;
            let assignee_id = actor_id.to_string();
            card_response(
                claimed,
                AgentCommandEffect::CardAssigned {
                    card_id,
                    assignee_id,
                },
            )
        }
        AgentCommand::CardMove {
            card_id,
            column_id,
            before_card_id,
        } => {
            let moved =
                Board::move_card_in(transaction, &card_id, &column_id, before_card_id.as_deref())
                    .await;
            let position = moved.as_ref().map_or(0, |card| card.position);
            card_response(
                moved,
                AgentCommandEffect::CardMoved {
                    card_id,
                    column_id,
                    position,
                },
            )
        }
        _ => unreachable!("execute dispatches only Board and Card commands here"),
    }
}

fn card_response(
    result: Result<CardView, BoardOperationError>,
    effect: AgentCommandEffect,
) -> Result<AgentCommandResponse, sqlx::Error> {
    match result {
        Ok(card) => Ok(AgentCommandResponse {
            result: AgentCommandResult::Card { card },
            effects: vec![effect],
        }),
        Err(error) => board_failure(error),
    }
}

fn board_failure(failure: BoardOperationError) -> Result<AgentCommandResponse, sqlx::Error> {
    match failure {
        BoardOperationError::Domain { code, message } => Ok(error(code, &message)),
        BoardOperationError::Database(error) => Err(error),
    }
}
