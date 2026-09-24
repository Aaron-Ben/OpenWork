//! `collab_room_open` 的房间快照（collaboration-desktop.md §4.2）：带作者信息的消息与说明行。
//! 只供 Desktop 读取。

use std::collections::HashSet;

use sqlx::{FromRow, PgPool};

use super::{
    agents::Agents,
    messages::{MessageRow, Messages},
    room_notes::{self, NoteMessage},
};
use crate::protocol::{RoomMessageView, RoomSnapshotView};

pub(crate) struct RoomSnapshots;

impl RoomSnapshots {
    /// 房间的完整快照；房间不存在时返回 `NOT_FOUND`。
    pub(crate) async fn open(
        pool: &PgPool,
        agents: &Agents,
        room_id: &str,
    ) -> Result<RoomSnapshotView, sqlx::Error> {
        let mut connection = pool.acquire().await?;
        let member_ids: Option<Vec<String>> = sqlx::query_scalar(
            "SELECT ARRAY(SELECT member.participant_id FROM collab_room_members member
                          WHERE member.room_id = room.id AND member.participant_id <> 'local-user'
                          ORDER BY member.participant_id)
             FROM collab_rooms room WHERE room.id = $1",
        )
        .bind(room_id)
        .fetch_optional(&mut *connection)
        .await?;
        let member_ids = member_ids
            .ok_or_else(|| sqlx::Error::Protocol("NOT_FOUND: Room does not exist".to_string()))?;
        let rows = sqlx::query_as::<_, AuthoredRow>(
            "SELECT message.id, message.room_id, message.sequence, message.author_id, message.body,
                    message.quoted_message_id, author.display_name AS author_name,
                    author.kind AS author_kind, profile.role AS author_role, message.kind,
                    to_char(message.created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS') || '+08:00'
                        AS created_at
             FROM collab_messages message
             JOIN collab_participants author ON author.id = message.author_id
             LEFT JOIN collab_agent_profiles profile ON profile.agent_id = message.author_id
             WHERE message.room_id = $1
             ORDER BY message.sequence",
        )
        .bind(room_id)
        .fetch_all(&mut *connection)
        .await?;
        let system_ids = rows
            .iter()
            .filter(|row| row.kind == "system")
            .map(|row| row.message.id.clone())
            .collect::<HashSet<_>>();
        let messages = authored_views(&mut connection, rows).await?;
        let triages = room_notes::triages_in(&mut connection, room_id).await?;
        // 成员变动的系统消息作者记为用户，但它不是人类发言，也不触发说明行（collaboration-desktop.md §7.3）。
        let note_messages = messages
            .iter()
            .filter(|message| !system_ids.contains(&message.message.id))
            .map(|message| NoteMessage {
                sequence: message.message.sequence,
                author_is_user: message.author_kind == "user",
                author_name: &message.author_name,
                body: &message.message.body,
            })
            .collect::<Vec<_>>();
        let agent_names = agents
            .list()
            .await?
            .into_iter()
            .filter(|record| member_ids.contains(&record.id))
            .map(|record| (record.id, record.display_name))
            .collect::<Vec<_>>();
        let notes = room_notes::notes(&note_messages, &triages, &agent_names);
        Ok(RoomSnapshotView {
            room_id: room_id.to_string(),
            messages,
            notes,
        })
    }
}

/// 经 `Messages::views` 补齐引用摘要，再带上作者信息与时间。
async fn authored_views(
    connection: &mut sqlx::PgConnection,
    rows: Vec<AuthoredRow>,
) -> Result<Vec<RoomMessageView>, sqlx::Error> {
    let (message_rows, authors): (Vec<_>, Vec<_>) = rows
        .into_iter()
        .map(|row| {
            (
                row.message,
                (
                    row.author_name,
                    row.author_kind,
                    row.author_role,
                    row.created_at,
                ),
            )
        })
        .unzip();
    let views = Messages::views(&mut *connection, message_rows).await?;
    Ok(views
        .into_iter()
        .zip(authors)
        .map(
            |(message, (author_name, author_kind, author_role, created_at))| RoomMessageView {
                message,
                author_name,
                author_kind,
                author_role,
                created_at,
            },
        )
        .collect())
}

#[derive(FromRow)]
struct AuthoredRow {
    #[sqlx(flatten)]
    message: MessageRow,
    author_name: String,
    author_kind: String,
    author_role: Option<String>,
    created_at: String,
    /// `normal` 或 `system`。
    kind: String,
}
