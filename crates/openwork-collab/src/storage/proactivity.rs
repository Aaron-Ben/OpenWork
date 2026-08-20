//! PostgreSQL reads and room markers used by autonomous scheduling.

use std::time::Duration;

use serde_json::json;

use super::{CollabStorage, StorageError, board::insert_system_message};
use crate::{
    model::{AgendaCandidate, AgendaCard, Agent, Message, ScannerRoomSnapshot, ScannerSnapshot},
    time::china_now,
};

impl CollabStorage {
    pub async fn scanner_agents(&self) -> Result<Vec<Agent>, StorageError> {
        Ok(self
            .agents()
            .await?
            .into_iter()
            .filter(|agent| agent.enabled && agent.scanner_enabled)
            .collect())
    }

    pub async fn create_direct_room(
        &self,
        first_participant: &str,
        second_participant: &str,
    ) -> Result<crate::model::Room, StorageError> {
        if first_participant == second_participant
            || first_participant.trim().is_empty()
            || second_participant.trim().is_empty()
        {
            return Err(StorageError::InvalidInput(
                "a direct room requires two distinct participants".to_string(),
            ));
        }
        let mut members = [first_participant, second_participant];
        members.sort();
        let direct_key = format!("{}:{}", members[0], members[1]);
        let id = format!("dm_{}", uuid::Uuid::new_v4().simple());
        let mut transaction = self.pool.begin().await?;
        let row = sqlx::query_as::<_, super::RoomRow>(
            "INSERT INTO collab_rooms (id, kind, direct_key)
             VALUES ($1, 'direct', $2)
             ON CONFLICT (direct_key) WHERE direct_key IS NOT NULL
             DO UPDATE SET updated_at = collab_rooms.updated_at
             RETURNING id, kind, title, next_seq",
        )
        .bind(id)
        .bind(direct_key)
        .fetch_one(&mut *transaction)
        .await?;
        for member in members {
            sqlx::query(
                "INSERT INTO collab_room_members (room_id, participant_id, last_read_seq)
                 VALUES ($1, $2, $3)
                 ON CONFLICT (room_id, participant_id) DO NOTHING",
            )
            .bind(&row.id)
            .bind(member)
            .bind(row.next_seq)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        Ok(row.into_room())
    }

    pub async fn agent_is_quiet(
        &self,
        agent_id: &str,
        quiet_for: Duration,
    ) -> Result<bool, StorageError> {
        let quiet_for = time::Duration::try_from(quiet_for).map_err(|_| {
            StorageError::InvalidInput("agenda quiet period is too large".to_string())
        })?;
        let cutoff = china_now() - quiet_for;
        Ok(sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(
                SELECT 1 FROM collab_agents a
                 WHERE a.id = $1 AND a.enabled = TRUE
                   AND a.created_at <= $2
                   AND NOT EXISTS (
                       SELECT 1 FROM collab_runs running
                        WHERE running.agent_id = a.id AND running.status = 'running'
                   )
                   AND COALESCE((
                       SELECT MAX(ended_at) FROM collab_runs finished
                        WHERE finished.agent_id = a.id
                   ), a.created_at) <= $2
             )",
        )
        .bind(agent_id)
        .bind(cutoff)
        .fetch_one(&self.pool)
        .await?)
    }

    pub async fn first_agent_room(
        &self,
        agent_id: &str,
    ) -> Result<Option<(String, i64)>, StorageError> {
        Ok(sqlx::query_as::<_, RoomAnchorRow>(
            "SELECT r.id AS room_id, r.next_seq
               FROM collab_room_members rm
               JOIN collab_rooms r ON r.id = rm.room_id
              WHERE rm.participant_id = $1 AND rm.muted = FALSE
              ORDER BY r.last_message_at DESC NULLS LAST, r.id
              LIMIT 1",
        )
        .bind(agent_id)
        .fetch_optional(&self.pool)
        .await?
        .map(|row| (row.room_id, row.next_seq)))
    }

    pub async fn agenda_candidates(
        &self,
        agent_id: &str,
    ) -> Result<Vec<AgendaCandidate>, StorageError> {
        let rooms = sqlx::query_as::<_, AgendaRoomRow>(
            "SELECT r.id AS room_id, r.next_seq,
                    (r.last_message_at IS NOT NULL
                     AND r.last_message_at <= (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
                         - INTERVAL '5 minutes'
                     AND r.last_message_at >= (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
                         - INTERVAL '6 hours') AS stalled
               FROM collab_room_members rm
               JOIN collab_rooms r ON r.id = rm.room_id
              WHERE rm.participant_id = $1 AND rm.muted = FALSE
                AND (
                    EXISTS (
                        SELECT 1 FROM collab_boards b
                        JOIN collab_board_columns bc ON bc.board_id = b.id
                        JOIN collab_cards c ON c.column_id = bc.id
                        WHERE b.room_id = r.id AND bc.is_done = FALSE
                          AND (
                            c.assignee_id = $1
                            OR strpos(lower(c.title), lower('@' || $1)) > 0
                            OR strpos(lower(COALESCE(c.description, '')), lower('@' || $1)) > 0
                          )
                    )
                    OR (
                        r.last_message_at IS NOT NULL
                        AND r.last_message_at <= (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
                            - INTERVAL '5 minutes'
                        AND r.last_message_at >= (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
                            - INTERVAL '6 hours'
                    )
                )
              ORDER BY r.last_message_at DESC NULLS LAST, r.id",
        )
        .bind(agent_id)
        .fetch_all(&self.pool)
        .await?;
        let mut candidates = Vec::with_capacity(rooms.len());
        for room in rooms {
            let cards = sqlx::query_as::<_, AgendaCardRow>(
                "SELECT c.id, c.title, c.description, c.assignee_id
                   FROM collab_boards b
                   JOIN collab_board_columns bc ON bc.board_id = b.id
                   JOIN collab_cards c ON c.column_id = bc.id
                  WHERE b.room_id = $1 AND bc.is_done = FALSE
                    AND (
                        c.assignee_id = $2
                        OR strpos(lower(c.title), lower('@' || $2)) > 0
                        OR strpos(lower(COALESCE(c.description, '')), lower('@' || $2)) > 0
                    )
                  ORDER BY (c.assignee_id = $2) DESC, c.updated_at, c.id",
            )
            .bind(&room.room_id)
            .bind(agent_id)
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(AgendaCardRow::into_card)
            .collect();
            let recent_messages = self.glance(&room.room_id, 20).await?.messages;
            candidates.push(AgendaCandidate {
                room_id: room.room_id,
                highest_sequence: room.next_seq,
                cards,
                stalled: room.stalled,
                recent_messages,
            });
        }
        candidates.sort_by_key(|candidate| (candidate.cards.is_empty(), candidate.room_id.clone()));
        Ok(candidates)
    }

    pub async fn scanner_snapshot(&self, agent_id: &str) -> Result<ScannerSnapshot, StorageError> {
        let rooms = sqlx::query_as::<_, ScannerRoomRow>(
            "SELECT r.id AS room_id,
                    COALESCE((
                        SELECT MAX(peer.sequence) FROM collab_messages peer
                         WHERE peer.room_id = r.id AND peer.author_id <> $1
                    ), 0) AS peer_seq
               FROM collab_room_members rm
               JOIN collab_rooms r ON r.id = rm.room_id
              WHERE rm.participant_id = $1 AND rm.muted = FALSE
                AND r.last_message_at >= (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
                    - INTERVAL '24 hours'
                AND (
                    SELECT count(*) FROM collab_messages m
                     WHERE m.room_id = r.id
                       AND m.created_at >= (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
                           - INTERVAL '24 hours'
                ) >= 8
              ORDER BY r.last_message_at DESC, r.id",
        )
        .bind(agent_id)
        .fetch_all(&self.pool)
        .await?;
        let mut snapshots = Vec::with_capacity(rooms.len());
        for room in rooms {
            snapshots.push(ScannerRoomSnapshot {
                recent_messages: self.glance(&room.room_id, 20).await?.messages,
                room_id: room.room_id,
                highest_sequence: room.peer_seq,
            });
        }
        Ok(ScannerSnapshot {
            agent_id: agent_id.to_string(),
            rooms: snapshots,
        })
    }

    pub async fn agent_direct_exchange(
        &self,
        room_id: &str,
        limit: u32,
    ) -> Result<Option<Vec<Message>>, StorageError> {
        let is_agent_direct = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(
                SELECT 1 FROM collab_rooms r
                 WHERE r.id = $1 AND r.kind = 'direct'
                   AND (SELECT count(*) FROM collab_room_members rm WHERE rm.room_id = r.id) = 2
                   AND (
                       SELECT count(*) FROM collab_room_members rm
                       JOIN collab_participants p ON p.id = rm.participant_id
                        WHERE rm.room_id = r.id AND p.kind = 'agent'
                   ) = 2
             )",
        )
        .bind(room_id)
        .fetch_one(&self.pool)
        .await?;
        if !is_agent_direct {
            return Ok(None);
        }
        Ok(Some(self.glance(room_id, limit).await?.messages))
    }

    pub async fn insert_proactive_marker(
        &self,
        room_id: &str,
        agent_id: &str,
        trigger: &str,
        reason: &str,
    ) -> Result<Message, StorageError> {
        if !matches!(trigger, "agenda" | "scanner") {
            return Err(StorageError::InvalidInput(format!(
                "invalid proactive trigger {trigger}"
            )));
        }
        if !self.is_member(room_id, agent_id).await? {
            return Err(StorageError::NotFound(format!(
                "Agent {agent_id} membership in room {room_id}"
            )));
        }
        let mut transaction = self.pool.begin().await?;
        let payload = json!({
            "type": "proactive_wake",
            "trigger": trigger,
            "agentId": agent_id,
            "reason": reason,
        });
        let message = insert_system_message(
            &mut transaction,
            room_id,
            agent_id,
            &format!("{agent_id} started proactively: {reason}"),
            payload,
        )
        .await?;
        transaction.commit().await?;
        Ok(message)
    }
}

#[derive(Debug, sqlx::FromRow)]
struct AgendaRoomRow {
    room_id: String,
    next_seq: i64,
    stalled: bool,
}

#[derive(Debug, sqlx::FromRow)]
struct RoomAnchorRow {
    room_id: String,
    next_seq: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct ScannerRoomRow {
    room_id: String,
    peer_seq: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct AgendaCardRow {
    id: String,
    title: String,
    description: Option<String>,
    assignee_id: Option<String>,
}

impl AgendaCardRow {
    fn into_card(self) -> AgendaCard {
        AgendaCard {
            id: self.id,
            title: self.title,
            description: self.description,
            assignee_id: self.assignee_id,
        }
    }
}
