//! Agent 静音自己所在的 Group（collaboration.md §10.1，Cumora `cli.ts` 的 `cmdMute` / `cmdFollow`）。
//! 收件箱与唤醒按 `collab_room_members.mute_expires_at` 判断是否静音，见 `messages.rs`。

use std::borrow::Cow;

use sqlx::{Postgres, Transaction};
use time::{OffsetDateTime, PrimitiveDateTime, UtcOffset, format_description::well_known::Rfc3339};

use crate::protocol::MuteView;

/// Longest `--for` mute (collaboration.md §10.1, Cumora `parseMuteUntil`: 1 minute to 90 days).
pub(crate) const MUTE_MAX_MINUTES: u32 = 90 * 24 * 60;

/// 库里的时间是东八区墙上时间（collaboration.md §13.1）。
const CHINA_OFFSET: UtcOffset = match UtcOffset::from_hms(8, 0, 0) {
    Ok(offset) => offset,
    Err(_) => panic!("+08:00 is a valid offset"),
};

/// 静音多久：一直静音、静音若干分钟，或静音到某个 RFC 3339 时刻。
pub(crate) enum MuteSpan<'a> {
    Indefinite,
    Minutes(u32),
    Until(&'a str),
}

pub(crate) enum MuteError {
    Domain {
        code: &'static str,
        message: Cow<'static, str>,
    },
    Database(sqlx::Error),
}

impl From<sqlx::Error> for MuteError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

pub(crate) struct RoomMutes;

impl RoomMutes {
    /// 把 `for_minutes` / `until` 解析成静音期限；两者同时给、分钟数越界、时间不合法或不在未来时返回
    /// `INVALID_ARGUMENT`。
    pub(crate) fn span<'a>(
        for_minutes: Option<u32>,
        until: Option<&'a str>,
    ) -> Result<MuteSpan<'a>, MuteError> {
        match (for_minutes, until) {
            (Some(_), Some(_)) => Err(invalid("use either --until or --for, not both")),
            (Some(minutes), None) if (1..=MUTE_MAX_MINUTES).contains(&minutes) => {
                Ok(MuteSpan::Minutes(minutes))
            }
            (Some(_), None) => Err(invalid(
                "--for duration must be between 1 minute and 90 days",
            )),
            (None, Some(until)) => Ok(MuteSpan::Until(until)),
            (None, None) => Ok(MuteSpan::Indefinite),
        }
    }

    /// 静音 `agent_id` 所在的 Group，并把它的 `last_read_seq` 推进到房间当前最后一条（封住未读尾巴）。
    /// Direct Room 返回 `INVALID_ARGUMENT`，不是成员返回 `NOT_FOUND`，两种情况都不写入。
    pub(crate) async fn mute_in(
        transaction: &mut Transaction<'_, Postgres>,
        agent_id: &str,
        room_id: &str,
        span: MuteSpan<'_>,
    ) -> Result<MuteView, MuteError> {
        let until = match span {
            MuteSpan::Until(value) => Some(future_china_time(value)?),
            _ => None,
        };
        let minutes = match span {
            MuteSpan::Minutes(minutes) => Some(minutes as i32),
            _ => None,
        };
        let kind: Option<String> = sqlx::query_scalar(
            "SELECT room.kind
             FROM collab_room_members member
             JOIN collab_rooms room ON room.id = member.room_id
             WHERE member.room_id = $1 AND member.participant_id = $2
             FOR UPDATE OF member",
        )
        .bind(room_id)
        .bind(agent_id)
        .fetch_optional(&mut **transaction)
        .await?;
        match kind.as_deref() {
            None => return Err(not_member(room_id)),
            Some("direct") => {
                return Err(invalid("direct rooms always deliver; mute a group instead"));
            }
            Some(_) => {}
        }
        let (title, expires_at): (Option<String>, Option<String>) = sqlx::query_as(
            "UPDATE collab_room_members member
             SET mute_expires_at = CASE
                     WHEN $3::INTEGER IS NOT NULL
                         THEN (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
                             + make_interval(mins => $3)
                     WHEN $4::TIMESTAMP IS NOT NULL THEN $4
                     ELSE 'infinity'::TIMESTAMP
                 END,
                 last_read_seq = GREATEST(member.last_read_seq, room.next_seq)
             FROM collab_rooms room
             WHERE member.room_id = room.id AND member.room_id = $1 AND member.participant_id = $2
             RETURNING room.title, CASE WHEN isfinite(member.mute_expires_at)
                 THEN to_char(member.mute_expires_at, 'YYYY-MM-DD\"T\"HH24:MI:SS') || '+08:00'
             END",
        )
        .bind(room_id)
        .bind(agent_id)
        .bind(minutes)
        .bind(until)
        .fetch_one(&mut **transaction)
        .await?;
        Ok(MuteView {
            room_id: room_id.to_string(),
            title,
            expires_at,
        })
    }

    /// 恢复接收；返回之前是否仍在静音。不是成员时返回 `NOT_FOUND`。
    pub(crate) async fn follow_in(
        transaction: &mut Transaction<'_, Postgres>,
        agent_id: &str,
        room_id: &str,
    ) -> Result<bool, MuteError> {
        let was_muted: Option<bool> = sqlx::query_scalar(
            "SELECT COALESCE(
                 mute_expires_at > (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'), FALSE
             )
             FROM collab_room_members
             WHERE room_id = $1 AND participant_id = $2
             FOR UPDATE",
        )
        .bind(room_id)
        .bind(agent_id)
        .fetch_optional(&mut **transaction)
        .await?;
        let Some(was_muted) = was_muted else {
            return Err(not_member(room_id));
        };
        sqlx::query(
            "UPDATE collab_room_members SET mute_expires_at = NULL
             WHERE room_id = $1 AND participant_id = $2",
        )
        .bind(room_id)
        .bind(agent_id)
        .execute(&mut **transaction)
        .await?;
        Ok(was_muted)
    }

    /// 仍在静音的 Group，按房间 id 排列。
    pub(crate) async fn list_in(
        transaction: &mut Transaction<'_, Postgres>,
        agent_id: &str,
    ) -> Result<Vec<MuteView>, sqlx::Error> {
        let rows: Vec<(String, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT member.room_id, room.title, CASE WHEN isfinite(member.mute_expires_at)
                 THEN to_char(member.mute_expires_at, 'YYYY-MM-DD\"T\"HH24:MI:SS') || '+08:00'
             END
             FROM collab_room_members member
             JOIN collab_rooms room ON room.id = member.room_id
             WHERE member.participant_id = $1
               AND member.mute_expires_at > (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
             ORDER BY member.room_id",
        )
        .bind(agent_id)
        .fetch_all(&mut **transaction)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(room_id, title, expires_at)| MuteView {
                room_id,
                title,
                expires_at,
            })
            .collect())
    }
}

/// 把 RFC 3339 时刻换成东八区墙上时间；不合法或不在未来时返回 `INVALID_ARGUMENT`。
fn future_china_time(value: &str) -> Result<PrimitiveDateTime, MuteError> {
    let instant =
        OffsetDateTime::parse(value, &Rfc3339).map_err(|_| invalid("invalid --until timestamp"))?;
    if instant <= OffsetDateTime::now_utc() {
        return Err(invalid("--until must be in the future"));
    }
    let local = instant.to_offset(CHINA_OFFSET);
    Ok(PrimitiveDateTime::new(local.date(), local.time()))
}

fn invalid(message: &'static str) -> MuteError {
    MuteError::Domain {
        code: "INVALID_ARGUMENT",
        message: Cow::Borrowed(message),
    }
}

fn not_member(room_id: &str) -> MuteError {
    MuteError::Domain {
        code: "NOT_FOUND",
        message: Cow::Owned(format!("you are not a member of {room_id}")),
    }
}
