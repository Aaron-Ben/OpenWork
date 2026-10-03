//! Desktop 的房间列表与置顶（collaboration-desktop.md §4.2、§4.2、§7.1）。Agent 命令的房间视图在
//! `rooms.rs`，不经过这里。房间里谁在工作由 Agent 的 `activity` 得出，不在这里下发。

use sqlx::{FromRow, PgPool, Postgres, Transaction};

use crate::protocol::{LastMessageView, RoomSummaryView};

/// Characters of the last message shown in the room list (collaboration-desktop.md §4.2).
const LAST_MESSAGE_MAX_CHARS: i32 = 80;

pub(crate) struct RoomSummaries;

impl RoomSummaries {
    /// 所有房间，按最近消息时间（没有消息时按创建时间）从新到旧；置顶由 Desktop 排到最前。
    pub(crate) async fn list(pool: &PgPool) -> Result<Vec<RoomSummaryView>, sqlx::Error> {
        sqlx::query_as::<_, SummaryRow>(
            "SELECT room.id, room.kind,
                    CASE WHEN room.kind = 'direct' THEN (
                        SELECT participant.display_name
                        FROM collab_room_members member
                        JOIN collab_participants participant
                          ON participant.id = member.participant_id
                        WHERE member.room_id = room.id
                          AND member.participant_id <> 'local-user'
                        ORDER BY participant.id LIMIT 1
                    ) ELSE room.title END AS title,
                    (SELECT COUNT(*) FROM collab_messages message
                     WHERE message.room_id = room.id
                       AND message.sequence > room.user_viewed_seq
                       AND message.author_id <> 'local-user'
                       AND message.kind = 'normal') AS unread_count,
                    last.author_name AS last_author_name,
                    last.body AS last_body,
                    to_char(room.last_message_at, 'YYYY-MM-DD\"T\"HH24:MI:SS') || '+08:00'
                        AS last_message_at,
                    EXISTS (SELECT 1 FROM collab_room_members member
                            WHERE member.room_id = room.id
                              AND member.participant_id = 'local-user') AS user_is_member,
                    ARRAY(SELECT member.participant_id FROM collab_room_members member
                          WHERE member.room_id = room.id
                          ORDER BY member.participant_id <> 'local-user', member.participant_id)
                        AS member_ids,
                    room.user_pinned_at IS NOT NULL AS pinned
             FROM collab_rooms room
             LEFT JOIN LATERAL (
                 SELECT author.display_name AS author_name, LEFT(message.body, $1) AS body
                 FROM collab_messages message
                 JOIN collab_participants author ON author.id = message.author_id
                 WHERE message.room_id = room.id AND message.kind = 'normal'
                 ORDER BY message.sequence DESC
                 LIMIT 1
             ) last ON TRUE
             ORDER BY COALESCE(room.last_message_at, room.created_at) DESC, room.id",
        )
        .bind(LAST_MESSAGE_MAX_CHARS)
        .fetch_all(pool)
        .await
        .map(|rows| rows.into_iter().map(RoomSummaryView::from).collect())
    }

    /// 置顶或取消置顶；已经是目标状态时不改置顶时间。房间不存在时返回 `NOT_FOUND`。
    pub(crate) async fn pin_in(
        transaction: &mut Transaction<'_, Postgres>,
        room_id: &str,
        pinned: bool,
    ) -> Result<(), sqlx::Error> {
        let updated = sqlx::query(
            "UPDATE collab_rooms
             SET user_pinned_at = CASE
                 WHEN NOT $2 THEN NULL
                 ELSE COALESCE(user_pinned_at, CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
             END
             WHERE id = $1",
        )
        .bind(room_id)
        .bind(pinned)
        .execute(&mut **transaction)
        .await?;
        if updated.rows_affected() == 0 {
            return Err(sqlx::Error::Protocol(
                "NOT_FOUND: Room does not exist".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(FromRow)]
struct SummaryRow {
    id: String,
    kind: String,
    title: Option<String>,
    unread_count: i64,
    last_author_name: Option<String>,
    last_body: Option<String>,
    last_message_at: Option<String>,
    user_is_member: bool,
    member_ids: Vec<String>,
    pinned: bool,
}

impl From<SummaryRow> for RoomSummaryView {
    fn from(row: SummaryRow) -> Self {
        let last_message = row
            .last_author_name
            .zip(row.last_body)
            .map(|(author_name, body)| LastMessageView { author_name, body });
        Self {
            id: row.id,
            kind: row.kind,
            title: row.title,
            unread_count: row.unread_count,
            last_message,
            last_message_at: row.last_message_at,
            user_is_member: row.user_is_member,
            member_ids: row.member_ids,
            pinned: row.pinned,
        }
    }
}
