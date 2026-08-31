use std::collections::BTreeSet;

use sqlx::{FromRow, PgPool, Postgres, Transaction};

use crate::protocol::{MessageView, ParticipantView, RoomView, entity_id};

#[derive(Clone)]
pub(crate) struct Rooms {
    pool: PgPool,
}

impl Rooms {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) async fn create_direct_in(
        transaction: &mut Transaction<'_, Postgres>,
        agent_id: &str,
    ) -> Result<RoomView, sqlx::Error> {
        let active: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM collab_agent_profiles
             WHERE agent_id = $1 AND archived_at IS NULL)",
        )
        .bind(agent_id)
        .fetch_one(&mut **transaction)
        .await?;
        if !active {
            return Err(protocol_error(
                "NOT_FOUND: Agent does not exist or is archived",
            ));
        }
        let (room_id, _) =
            get_or_create_direct_room(transaction, "local-user", agent_id, "local-user").await?;
        Ok(RoomView {
            id: room_id,
            kind: "direct".to_string(),
            title: None,
        })
    }

    pub(crate) async fn create_group_in(
        transaction: &mut Transaction<'_, Postgres>,
        title: &str,
        agent_ids: &[String],
    ) -> Result<RoomView, sqlx::Error> {
        let title = title.trim();
        if title.is_empty() || title.len() > 120 {
            return Err(protocol_error(
                "INVALID_ARGUMENT: group title must be 1..120 bytes",
            ));
        }
        let agent_ids: BTreeSet<String> = agent_ids
            .iter()
            .filter(|id| !id.is_empty())
            .cloned()
            .collect();
        if agent_ids.len() < 2 {
            return Err(protocol_error(
                "INVALID_ARGUMENT: a user group needs at least two Agents",
            ));
        }
        let agent_ids: Vec<String> = agent_ids.into_iter().collect();
        let valid_agent_ids: Vec<String> = sqlx::query_scalar(
            "SELECT agent_id FROM collab_agent_profiles
             WHERE agent_id = ANY($1) AND archived_at IS NULL ORDER BY agent_id",
        )
        .bind(&agent_ids)
        .fetch_all(&mut **transaction)
        .await?;
        if valid_agent_ids != agent_ids {
            return Err(protocol_error(
                "NOT_FOUND: one or more selected Agents do not exist or are archived",
            ));
        }
        let room_id = entity_id("room");
        sqlx::query(
            "INSERT INTO collab_rooms (id, kind, title, created_by)
             VALUES ($1, 'group', $2, 'local-user')",
        )
        .bind(&room_id)
        .bind(title)
        .execute(&mut **transaction)
        .await?;
        let mut member_ids = Vec::with_capacity(agent_ids.len() + 1);
        member_ids.push("local-user".to_string());
        member_ids.extend(agent_ids);
        for member_id in member_ids {
            sqlx::query(
                "INSERT INTO collab_room_members (room_id, participant_id, last_read_seq)
                 VALUES ($1, $2, 0)",
            )
            .bind(&room_id)
            .bind(member_id)
            .execute(&mut **transaction)
            .await?;
        }
        Ok(RoomView {
            id: room_id,
            kind: "group".to_string(),
            title: Some(title.to_string()),
        })
    }

    pub(crate) async fn list(&self) -> Result<Vec<RoomView>, sqlx::Error> {
        sqlx::query_as::<_, RoomRow>(
            "SELECT room.id, room.kind,
                    CASE WHEN room.kind = 'direct' THEN (
                        SELECT participant.display_name
                        FROM collab_room_members member
                        JOIN collab_participants participant
                          ON participant.id = member.participant_id
                        WHERE member.room_id = room.id
                          AND member.participant_id <> 'local-user'
                        ORDER BY participant.id LIMIT 1
                    ) ELSE room.title END AS title
             FROM collab_rooms room
             ORDER BY COALESCE(room.last_message_at, room.created_at) DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(RoomView::from).collect())
    }

    pub(crate) async fn list_for_agent_in(
        transaction: &mut Transaction<'_, Postgres>,
        agent_id: &str,
    ) -> Result<Vec<RoomView>, sqlx::Error> {
        sqlx::query_as::<_, RoomRow>(
            "SELECT room.id, room.kind, room.title
             FROM collab_rooms room
             JOIN collab_room_members member
               ON member.room_id = room.id AND member.participant_id = $1
             ORDER BY COALESCE(room.last_message_at, room.created_at) DESC, room.id",
        )
        .bind(agent_id)
        .fetch_all(&mut **transaction)
        .await
        .map(|rows| rows.into_iter().map(RoomView::from).collect())
    }

    pub(crate) async fn list_members_for_agent_in(
        transaction: &mut Transaction<'_, Postgres>,
        agent_id: &str,
        room_id: &str,
    ) -> Result<Vec<ParticipantView>, sqlx::Error> {
        let member: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM collab_room_members
                WHERE room_id = $1 AND participant_id = $2
             )",
        )
        .bind(room_id)
        .bind(agent_id)
        .fetch_one(&mut **transaction)
        .await?;
        if !member {
            return Err(protocol_error(
                "NOT_FOUND: Room is not visible to this Agent",
            ));
        }
        sqlx::query_as::<_, ParticipantRow>(
            "SELECT participant.id, participant.kind, participant.display_name
             FROM collab_room_members member
             JOIN collab_participants participant ON participant.id = member.participant_id
             WHERE member.room_id = $1
             ORDER BY CASE WHEN participant.kind = 'user' THEN 0 ELSE 1 END,
                      participant.display_name, participant.id",
        )
        .bind(room_id)
        .fetch_all(&mut **transaction)
        .await
        .map(|rows| rows.into_iter().map(ParticipantView::from).collect())
    }

    pub(crate) async fn list_members(
        &self,
        room_id: &str,
    ) -> Result<Vec<ParticipantView>, sqlx::Error> {
        let visible: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM collab_rooms room
                JOIN collab_room_members viewer
                  ON viewer.room_id = room.id AND viewer.participant_id = 'local-user'
                WHERE room.id = $1 AND room.kind = 'group'
             )",
        )
        .bind(room_id)
        .fetch_one(&self.pool)
        .await?;
        if !visible {
            return Err(protocol_error("NOT_FOUND: group is not visible"));
        }
        sqlx::query_as::<_, ParticipantRow>(
            "SELECT participant.id, participant.kind, participant.display_name
             FROM collab_room_members member
             JOIN collab_participants participant ON participant.id = member.participant_id
             WHERE member.room_id = $1
             ORDER BY CASE WHEN participant.id = 'local-user' THEN 0 ELSE 1 END,
                      participant.display_name, participant.id",
        )
        .bind(room_id)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(ParticipantView::from).collect())
    }

    pub(crate) async fn list_members_in(
        transaction: &mut Transaction<'_, Postgres>,
        room_id: &str,
    ) -> Result<Vec<ParticipantView>, sqlx::Error> {
        let visible: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM collab_rooms room
                JOIN collab_room_members viewer
                  ON viewer.room_id = room.id AND viewer.participant_id = 'local-user'
                WHERE room.id = $1 AND room.kind = 'group'
             )",
        )
        .bind(room_id)
        .fetch_one(&mut **transaction)
        .await?;
        if !visible {
            return Err(protocol_error("NOT_FOUND: group is not visible"));
        }
        sqlx::query_as::<_, ParticipantRow>(
            "SELECT participant.id, participant.kind, participant.display_name
             FROM collab_room_members member
             JOIN collab_participants participant ON participant.id = member.participant_id
             WHERE member.room_id = $1
             ORDER BY CASE WHEN participant.id = 'local-user' THEN 0 ELSE 1 END,
                      participant.display_name, participant.id",
        )
        .bind(room_id)
        .fetch_all(&mut **transaction)
        .await
        .map(|rows| rows.into_iter().map(ParticipantView::from).collect())
    }

    pub(crate) async fn agent_ids(&self, room_id: &str) -> Result<Vec<String>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT agent.agent_id
             FROM collab_room_members member
             JOIN collab_agent_profiles agent ON agent.agent_id = member.participant_id
             WHERE member.room_id = $1 AND agent.archived_at IS NULL
             ORDER BY agent.agent_id",
        )
        .bind(room_id)
        .fetch_all(&self.pool)
        .await
    }

    pub(crate) async fn change_member_in(
        transaction: &mut Transaction<'_, Postgres>,
        room_id: &str,
        agent_id: &str,
        adding: bool,
    ) -> Result<(Vec<ParticipantView>, Option<MessageView>), sqlx::Error> {
        let current_sequence: Option<i64> = sqlx::query_scalar(
            "SELECT room.next_seq
             FROM collab_rooms room
             JOIN collab_room_members viewer
               ON viewer.room_id = room.id AND viewer.participant_id = 'local-user'
             WHERE room.id = $1 AND room.kind = 'group'
             FOR UPDATE OF room",
        )
        .bind(room_id)
        .fetch_optional(&mut **transaction)
        .await?;
        let Some(current_sequence) = current_sequence else {
            return Err(protocol_error("NOT_FOUND: group is not visible"));
        };
        let agent_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM collab_agent_profiles
                WHERE agent_id = $1 AND ($2 = FALSE OR archived_at IS NULL)
             )",
        )
        .bind(agent_id)
        .bind(adding)
        .fetch_one(&mut **transaction)
        .await?;
        if !agent_exists {
            return Err(protocol_error(
                "NOT_FOUND: Agent does not exist or is archived",
            ));
        }
        let is_member: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM collab_room_members
                WHERE room_id = $1 AND participant_id = $2
             )",
        )
        .bind(room_id)
        .bind(agent_id)
        .fetch_one(&mut **transaction)
        .await?;
        if is_member == adding {
            return Ok((Self::list_members_in(transaction, room_id).await?, None));
        }
        if adding {
            sqlx::query(
                "INSERT INTO collab_room_members (room_id, participant_id, last_read_seq)
                 VALUES ($1, $2, $3)",
            )
            .bind(room_id)
            .bind(agent_id)
            .bind(current_sequence)
            .execute(&mut **transaction)
            .await?;
        }
        let message_id = entity_id("msg");
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
        let action = if adding { "invited" } else { "removed" };
        let body = format!("local-user {action} {agent_id}");
        sqlx::query(
            "INSERT INTO collab_messages (id, room_id, sequence, author_id, kind, body)
             VALUES ($1, $2, $3, 'local-user', 'system', $4)",
        )
        .bind(&message_id)
        .bind(room_id)
        .bind(sequence)
        .bind(&body)
        .execute(&mut **transaction)
        .await?;
        if !adding {
            sqlx::query(
                "DELETE FROM collab_room_members WHERE room_id = $1 AND participant_id = $2",
            )
            .bind(room_id)
            .bind(agent_id)
            .execute(&mut **transaction)
            .await?;
        }
        let message = MessageView {
            id: message_id,
            room_id: room_id.to_string(),
            sequence,
            author_id: "local-user".to_string(),
            body,
        };
        Ok((
            Self::list_members_in(transaction, room_id).await?,
            Some(message),
        ))
    }
}

pub(crate) async fn get_or_create_direct_room(
    transaction: &mut Transaction<'_, Postgres>,
    first_participant: &str,
    second_participant: &str,
    created_by: &str,
) -> Result<(String, bool), sqlx::Error> {
    if first_participant == second_participant {
        return Err(protocol_error(
            "INVALID_ARGUMENT: a direct room needs two participants",
        ));
    }
    let (left, right) = if first_participant < second_participant {
        (first_participant, second_participant)
    } else {
        (second_participant, first_participant)
    };
    let direct_key = format!("{left}|{right}");
    let proposed_room_id = entity_id("room");
    let inserted: Option<String> = sqlx::query_scalar(
        "INSERT INTO collab_rooms (id, kind, direct_key, created_by)
         VALUES ($1, 'direct', $2, $3)
         ON CONFLICT (direct_key) WHERE direct_key IS NOT NULL DO NOTHING
         RETURNING id",
    )
    .bind(&proposed_room_id)
    .bind(&direct_key)
    .bind(created_by)
    .fetch_optional(&mut **transaction)
    .await?;
    if let Some(room_id) = inserted {
        sqlx::query(
            "INSERT INTO collab_room_members (room_id, participant_id, last_read_seq)
             VALUES ($1, $2, 0), ($1, $3, 0)",
        )
        .bind(&room_id)
        .bind(left)
        .bind(right)
        .execute(&mut **transaction)
        .await?;
        Ok((room_id, true))
    } else {
        let room_id =
            sqlx::query_scalar("SELECT id FROM collab_rooms WHERE direct_key = $1 FOR UPDATE")
                .bind(direct_key)
                .fetch_one(&mut **transaction)
                .await?;
        Ok((room_id, false))
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

#[derive(FromRow)]
struct RoomRow {
    id: String,
    kind: String,
    title: Option<String>,
}

impl From<RoomRow> for RoomView {
    fn from(row: RoomRow) -> Self {
        Self {
            id: row.id,
            kind: row.kind,
            title: row.title,
        }
    }
}

fn protocol_error(message: &str) -> sqlx::Error {
    sqlx::Error::Protocol(message.to_string())
}
