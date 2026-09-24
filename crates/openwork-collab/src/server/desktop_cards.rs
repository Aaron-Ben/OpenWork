//! Desktop 用户的卡片命令（collaboration.md §11.2）：建卡、编辑、移动、改派与删除。改派与新增的
//! `@` 写入卡片唤醒（§11.4），发起者是 `local-user`，不受每分钟限额约束。

use sqlx::{Postgres, Transaction};

use super::{
    board::{Board, NewCard},
    card_wakes::CardWakes,
    desktop_commands::{PostCommitEffect, board_effect, board_error, protocol_error},
};
use crate::protocol::{CardChangeView, CardView, DesktopCommand, DesktopCommandResult};

type Mutation = (DesktopCommandResult, Vec<PostCommitEffect>);

/// 执行一条卡片命令；不是卡片命令时返回 `INVALID_ARGUMENT`。
pub(super) async fn card_command_in(
    transaction: &mut Transaction<'_, Postgres>,
    command: DesktopCommand,
) -> Result<Mutation, sqlx::Error> {
    match command {
        DesktopCommand::CreateCard {
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
                actor_id: "local-user",
            };
            let card = Board::create_card_in(transaction, new_card)
                .await
                .map_err(board_error)?;
            changed(transaction, None, card).await
        }
        DesktopCommand::UpdateCard {
            card_id,
            title,
            description,
        } => {
            if title.is_none() && description.is_none() {
                return Err(protocol_error(
                    "INVALID_ARGUMENT: nothing to update; pass a title or a description",
                ));
            }
            let edit = Board::update_card_in(
                transaction,
                &card_id,
                title.as_deref(),
                description.as_deref(),
            )
            .await
            .map_err(board_error)?;
            changed(transaction, Some(&edit.before), edit.after).await
        }
        DesktopCommand::AssignCard {
            card_id,
            assignee_id,
        } => {
            let edit = Board::assign_card_in(transaction, &card_id, assignee_id.as_deref())
                .await
                .map_err(board_error)?;
            changed(transaction, Some(&edit.before), edit.after).await
        }
        DesktopCommand::MoveCard {
            card_id,
            column_id,
            before_card_id,
        } => {
            let card =
                Board::move_card_in(transaction, &card_id, &column_id, before_card_id.as_deref())
                    .await
                    .map_err(board_error)?;
            let effect = board_effect(Some(card.board_id.clone()));
            let change = CardChangeView {
                card,
                woken_agent_ids: Vec::new(),
            };
            Ok((DesktopCommandResult::Card(change), vec![effect]))
        }
        DesktopCommand::DeleteCard { card_id } => {
            let board_id = Board::delete_card_in(transaction, &card_id)
                .await
                .map_err(board_error)?;
            Ok((
                DesktopCommandResult::Deleted { entity_id: card_id },
                vec![board_effect(Some(board_id))],
            ))
        }
        _ => Err(protocol_error("INVALID_ARGUMENT: not a card command")),
    }
}

/// 卡片修改后的结果：`before` 为 `None` 表示新建。改派与新增的 `@` 写入卡片唤醒，被叫醒的 Agent
/// 随结果返回，界面据此提示“已通知 <名字>”。
async fn changed(
    transaction: &mut Transaction<'_, Postgres>,
    before: Option<&CardView>,
    after: CardView,
) -> Result<Mutation, sqlx::Error> {
    let mut effects = vec![board_effect(Some(after.board_id.clone()))];
    let mut woken_agent_ids = Vec::new();
    for (agent_id, reason) in
        CardWakes::targets_in(transaction, before, &after, "local-user").await?
    {
        CardWakes::record_in(transaction, &after.id, &agent_id, reason).await?;
        effects.push(PostCommitEffect::CardWakeQueued {
            agent_id: agent_id.clone(),
            card_id: after.id.clone(),
        });
        woken_agent_ids.push(agent_id);
    }
    let change = CardChangeView {
        card: after,
        woken_agent_ids,
    };
    Ok((DesktopCommandResult::Card(change), effects))
}
