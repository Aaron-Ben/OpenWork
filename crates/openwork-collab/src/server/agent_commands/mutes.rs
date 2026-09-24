//! Agent 的静音命令（collaboration.md §10.1）：`mute`、`mute list`、`follow`。

use sqlx::{Postgres, Transaction};

use super::{error, success};
use crate::protocol::{AgentCommand, AgentCommandResponse, AgentCommandResult};
use crate::server::{
    auth::AgentClaims,
    room_mutes::{MuteError, RoomMutes},
};

/// 执行一条静音命令。`execute` 只把这三个变体分派到这里。
pub(super) async fn mute_command(
    transaction: &mut Transaction<'_, Postgres>,
    claims: &AgentClaims,
    command: AgentCommand,
) -> Result<AgentCommandResponse, sqlx::Error> {
    let agent_id = claims.sub.as_str();
    let outcome = match command {
        AgentCommand::Mute {
            room_id,
            for_minutes,
            until,
        } => match RoomMutes::span(for_minutes, until.as_deref()) {
            Ok(span) => RoomMutes::mute_in(transaction, agent_id, &room_id, span)
                .await
                .map(|mute| AgentCommandResult::Muted {
                    participant_id: agent_id.to_string(),
                    mute,
                }),
            Err(failure) => Err(failure),
        },
        AgentCommand::MuteList => Ok(AgentCommandResult::Mutes {
            mutes: RoomMutes::list_in(transaction, agent_id).await?,
        }),
        AgentCommand::Follow { room_id } => RoomMutes::follow_in(transaction, agent_id, &room_id)
            .await
            .map(|was_muted| AgentCommandResult::Followed { room_id, was_muted }),
        _ => unreachable!("execute dispatches only mute commands here"),
    };
    match outcome {
        Ok(result) => Ok(success(result)),
        Err(MuteError::Domain { code, message }) => Ok(error(code, &message)),
        Err(MuteError::Database(failure)) => Err(failure),
    }
}
