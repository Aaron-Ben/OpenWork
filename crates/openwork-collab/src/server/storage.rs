use std::{collections::BTreeMap, fmt::Write as _, time::Duration};

use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::protocol::{
    AgentAssignment, AgentView, BoardColumnView, BoardView, COLLAB_PROTOCOL_VERSION, CardView,
    ComputerStatus, ComputerView, DeliveryRange, EngineStatus, FinishRunRequest, HeartbeatRequest,
    InboxResponse, LocalComputerRegistration, MessageView, RoomView, RunSummaryView, RunView,
    TriageReportRequest, TriggerEnvelope,
};

use super::auth::AgentClaims;
use super::rooms::get_or_create_direct_room;

#[derive(Clone)]
pub struct CollaborationStore {
    pool: PgPool,
}

impl CollaborationStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn ensure_local_computer(
        &self,
        runtime_base_url: &str,
    ) -> Result<LocalComputerRegistration, sqlx::Error> {
        let token = device_token();
        let credential_hash = token_hash(&token);
        let mut transaction = self.pool.begin().await?;
        let inserted = sqlx::query_scalar::<_, String>(
            "INSERT INTO collab_computers (id, name, credential_hash)
             VALUES ('local', 'This Mac', $1)
             ON CONFLICT (id) DO NOTHING
             RETURNING id",
        )
        .bind(credential_hash)
        .fetch_optional(&mut *transaction)
        .await?
        .is_some();
        sqlx::query(
            "INSERT INTO collab_computer_engines (computer_id, engine_id)
             VALUES ('local', 'opencode')
             ON CONFLICT (computer_id, engine_id) DO NOTHING",
        )
        .execute(&mut *transaction)
        .await?;
        let row = sqlx::query_as::<_, ComputerRow>(
            "SELECT c.id, c.name, c.status, c.daemon_generation,
                    e.engine_id, e.status AS engine_status
             FROM collab_computers c
             JOIN collab_computer_engines e ON e.computer_id = c.id
             WHERE c.id = 'local' AND e.engine_id = 'opencode'",
        )
        .fetch_one(&mut *transaction)
        .await?;
        transaction.commit().await?;

        Ok(LocalComputerRegistration {
            computer: ComputerView {
                id: row.id,
                name: row.name,
                status: parse_computer_status(&row.status),
                engine_id: row.engine_id,
                engine_status: parse_engine_status(&row.engine_status),
                generation: row.daemon_generation,
            },
            runtime_base_url: runtime_base_url.to_string(),
            device_token: inserted.then_some(token),
        })
    }

    pub async fn create_agent(
        &self,
        id: &str,
        display_name: &str,
        system_prompt: &str,
        model: &str,
    ) -> Result<AgentView, sqlx::Error> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO collab_participants (id, kind, display_name)
             VALUES ($1, 'agent', $2)",
        )
        .bind(id)
        .bind(display_name)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO collab_agents (
                id, computer_id, system_prompt, engine_id, model
             ) VALUES ($1, 'local', $2, 'opencode', $3)",
        )
        .bind(id)
        .bind(system_prompt)
        .bind(model)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(AgentView {
            id: id.to_string(),
            display_name: display_name.to_string(),
            system_prompt: system_prompt.to_string(),
            engine_id: "opencode".to_string(),
            model: model.to_string(),
            config_version: 1,
            enabled: true,
            scanner_enabled: false,
        })
    }

    pub async fn set_agent_proactivity(
        &self,
        agent_id: &str,
        enabled: bool,
    ) -> Result<AgentView, sqlx::Error> {
        let row = sqlx::query_as::<_, AgentViewRow>(
            "UPDATE collab_agents
             SET scanner_enabled = $2, config_version = config_version + 1,
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             WHERE id = $1
             RETURNING id,
                 (SELECT display_name FROM collab_participants WHERE id = $1) AS display_name,
                 system_prompt, engine_id, model, config_version, enabled, scanner_enabled",
        )
        .bind(agent_id)
        .bind(enabled)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.into())
    }

    pub async fn create_direct_room(&self, agent_id: &str) -> Result<RoomView, sqlx::Error> {
        let mut transaction = self.pool.begin().await?;
        let (room_id, _) = get_or_create_direct_room(&mut transaction, "user", agent_id).await?;
        transaction.commit().await?;
        Ok(RoomView {
            id: room_id,
            kind: "direct".to_string(),
            title: None,
        })
    }

    pub async fn list_agents(&self) -> Result<Vec<AgentView>, sqlx::Error> {
        sqlx::query_as::<_, AgentViewRow>(
            "SELECT a.id, p.display_name, a.system_prompt, a.engine_id,
                    a.model, a.config_version, a.enabled, a.scanner_enabled
             FROM collab_agents a
             JOIN collab_participants p ON p.id = a.id
             ORDER BY a.id",
        )
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(AgentView::from).collect())
    }

    pub async fn create_board(&self, room_id: &str, title: &str) -> Result<BoardView, sqlx::Error> {
        if title.trim().is_empty() || title.len() > 200 {
            return Err(protocol_error(
                "INVALID_ARGUMENT: board title must be 1..200 bytes",
            ));
        }
        let board_id = format!("board_{}", Uuid::new_v4().simple());
        let mut transaction = self.pool.begin().await?;
        sqlx::query("INSERT INTO collab_boards (id, room_id, title) VALUES ($1, $2, $3)")
            .bind(&board_id)
            .bind(room_id)
            .bind(title.trim())
            .execute(&mut *transaction)
            .await?;
        for (position, title, is_done) in [
            (0_i32, "To do", false),
            (1, "Doing", false),
            (2, "Done", true),
        ] {
            sqlx::query(
                "INSERT INTO collab_board_columns (id, board_id, title, position, is_done)
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(format!("column_{}", Uuid::new_v4().simple()))
            .bind(&board_id)
            .bind(title)
            .bind(position)
            .bind(is_done)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        self.board(&board_id).await
    }

    pub async fn list_boards(&self) -> Result<Vec<BoardView>, sqlx::Error> {
        let ids: Vec<String> =
            sqlx::query_scalar("SELECT id FROM collab_boards ORDER BY created_at, id")
                .fetch_all(&self.pool)
                .await?;
        let mut boards = Vec::with_capacity(ids.len());
        for id in ids {
            boards.push(self.board(&id).await?);
        }
        Ok(boards)
    }

    pub async fn list_runs(&self, limit: u32) -> Result<Vec<RunSummaryView>, sqlx::Error> {
        let limit = i64::from(limit.clamp(1, 200));
        sqlx::query_as::<_, RunSummaryRow>(
            "SELECT id, agent_id, trigger, status, outcome, room_id, focus_card_id,
                    trigger_reason,
                    to_char(started_at, 'YYYY-MM-DD\"T\"HH24:MI:SS') || '+08:00' AS started_at
             FROM collab_runs ORDER BY started_at DESC, id DESC LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(RunSummaryView::from).collect())
    }

    async fn board(&self, board_id: &str) -> Result<BoardView, sqlx::Error> {
        let (id, room_id, title): (String, String, String) =
            sqlx::query_as("SELECT id, room_id, title FROM collab_boards WHERE id = $1")
                .bind(board_id)
                .fetch_one(&self.pool)
                .await?;
        let columns = sqlx::query_as::<_, BoardColumnRow>(
            "SELECT id, title, position, is_done
             FROM collab_board_columns WHERE board_id = $1 ORDER BY position, id",
        )
        .bind(board_id)
        .fetch_all(&self.pool)
        .await?;
        let mut views = Vec::with_capacity(columns.len());
        for column in columns {
            let cards = sqlx::query_as::<_, CardRow>(
                "SELECT id, title, description, position, assignee_id, claimed_by
                 FROM collab_cards WHERE board_id = $1 AND column_id = $2
                 ORDER BY position, id",
            )
            .bind(board_id)
            .bind(&column.id)
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(CardView::from)
            .collect();
            views.push(BoardColumnView {
                id: column.id,
                title: column.title,
                position: column.position,
                is_done: column.is_done,
                cards,
            });
        }
        Ok(BoardView {
            id,
            room_id,
            title,
            columns: views,
        })
    }

    pub async fn list_rooms(&self) -> Result<Vec<RoomView>, sqlx::Error> {
        sqlx::query_as::<_, RoomRow>(
            "SELECT id, kind, title FROM collab_rooms
             ORDER BY COALESCE(last_message_at, created_at) DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(RoomView::from).collect())
    }

    pub async fn send_user_message(
        &self,
        room_id: &str,
        body: &str,
    ) -> Result<MessageView, sqlx::Error> {
        self.insert_message(room_id, "user", body).await
    }

    pub async fn list_messages(&self, room_id: &str) -> Result<Vec<MessageView>, sqlx::Error> {
        sqlx::query_as::<_, MessageRow>(
            "SELECT id, room_id, sequence, author_id, body
             FROM collab_messages WHERE room_id = $1 ORDER BY sequence",
        )
        .bind(room_id)
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(MessageView::from).collect())
    }

    pub async fn wake_recipients(
        &self,
        message_id: &str,
        room_id: &str,
        author_id: &str,
    ) -> Result<Vec<String>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT rm.participant_id
             FROM collab_room_members rm
             JOIN collab_rooms r ON r.id = rm.room_id
             JOIN collab_agents a ON a.id = rm.participant_id
             JOIN collab_messages message
               ON message.id = $1 AND message.room_id = rm.room_id
             WHERE rm.room_id = $2 AND rm.participant_id <> $3 AND a.enabled
               AND (
                   NOT rm.muted OR r.kind = 'direct' OR (
                       message.kind = 'normal'
                       AND message.body ~ (
                           '(^|[^A-Za-z0-9_])@' || rm.participant_id ||
                           '([^A-Za-z0-9_]|$)'
                       )
                   )
               )
             ORDER BY rm.participant_id",
        )
        .bind(message_id)
        .bind(room_id)
        .bind(author_id)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn room_agent_ids(&self, room_id: &str) -> Result<Vec<String>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT agent.id
             FROM collab_room_members member
             JOIN collab_agents agent ON agent.id = member.participant_id
             WHERE member.room_id = $1 AND agent.enabled
             ORDER BY agent.id",
        )
        .bind(room_id)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn start_computer(&self, device_token: &str) -> Result<i64, sqlx::Error> {
        let mut transaction = self.pool.begin().await?;
        let row: (String, i64) = sqlx::query_as(
            "SELECT credential_hash, daemon_generation
             FROM collab_computers WHERE id = 'local' FOR UPDATE",
        )
        .fetch_one(&mut *transaction)
        .await?;
        authenticate_hash(device_token, &row.0)?;
        let generation = row.1 + 1;
        sqlx::query(
            "UPDATE collab_runs
             SET status = 'interrupted',
                 ended_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
                 error_code = 'FENCED',
                 error_message = 'a newer local daemon generation started'
             WHERE computer_id = 'local' AND status = 'running'",
        )
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "UPDATE collab_computers
             SET daemon_generation = $1, status = 'online',
                 last_seen_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             WHERE id = 'local'",
        )
        .bind(generation)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(generation)
    }

    pub async fn heartbeat(
        &self,
        device_token: &str,
        heartbeat: &HeartbeatRequest,
    ) -> Result<(), sqlx::Error> {
        if heartbeat.protocol_version != COLLAB_PROTOCOL_VERSION
            || heartbeat.engine.engine_id != "opencode"
        {
            return Err(protocol_error(
                "PROTOCOL_MISMATCH: unsupported runtime payload",
            ));
        }
        let engine_status = engine_status_name(heartbeat.engine.status);
        let checked = engine_status != "unknown";
        let computer_status = match heartbeat.status {
            ComputerStatus::Online => "online",
            ComputerStatus::Offline => "offline",
        };
        let mut transaction = self.pool.begin().await?;
        self.authorize_device_transaction(&mut transaction, device_token, heartbeat.generation)
            .await?;
        sqlx::query(
            "UPDATE collab_computers
             SET status = $4,
                 last_seen_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
                 daemon_version = $1, daemon_supervised = $2,
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             WHERE id = 'local' AND daemon_generation = $3",
        )
        .bind(&heartbeat.daemon_version)
        .bind(heartbeat.supervised)
        .bind(heartbeat.generation)
        .bind(computer_status)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "UPDATE collab_computer_engines
             SET status = $1,
                 checked_at = CASE WHEN $2 THEN CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
                                   ELSE NULL END
             WHERE computer_id = 'local' AND engine_id = 'opencode'",
        )
        .bind(engine_status)
        .bind(checked)
        .execute(&mut *transaction)
        .await?;
        if heartbeat.status == ComputerStatus::Offline {
            sqlx::query(
                "UPDATE collab_runs
                 SET status = 'interrupted',
                     ended_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
                     heartbeat_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
                     error_code = 'COMPUTER_OFFLINE',
                     error_message = 'Local Computer stopped before the run finished'
                 WHERE computer_id = 'local' AND computer_generation = $1
                   AND status = 'running'",
            )
            .bind(heartbeat.generation)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await
    }

    pub async fn sweep_offline_computer(&self, lease: Duration) -> Result<bool, sqlx::Error> {
        let mut transaction = self.pool.begin().await?;
        let result = sqlx::query(
            "UPDATE collab_computers
             SET status = 'offline',
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             WHERE id = 'local' AND status = 'online'
               AND last_seen_at < (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
                   - ($1::double precision * INTERVAL '1 second')",
        )
        .bind(lease.as_secs_f64())
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() > 0 {
            sqlx::query(
                "UPDATE collab_runs
                 SET status = 'interrupted',
                     ended_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
                     heartbeat_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
                     error_code = 'COMPUTER_OFFLINE',
                     error_message = 'Local Computer heartbeat lease expired'
                 WHERE computer_id = 'local' AND status = 'running'",
            )
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn roster(
        &self,
        device_token: &str,
        generation: i64,
    ) -> Result<Vec<AgentAssignment>, sqlx::Error> {
        self.authorize_device(device_token, generation).await?;
        sqlx::query_as::<_, AssignmentRow>(
            "SELECT a.id, p.display_name, a.role, a.bio, a.system_prompt,
                    a.engine_id, a.model, COALESCE(a.fast_model, a.model) AS fast_model,
                    a.config_version, a.scanner_enabled
             FROM collab_agents a
             JOIN collab_participants p ON p.id = a.id
             WHERE a.computer_id = 'local' AND a.enabled
             ORDER BY a.id",
        )
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(AgentAssignment::from).collect())
    }

    pub async fn authorize_agent_token(
        &self,
        device_token: &str,
        generation: i64,
        agent_id: &str,
    ) -> Result<(), sqlx::Error> {
        self.authorize_device(device_token, generation).await?;
        let available: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM collab_agents a
                JOIN collab_computer_engines e
                  ON e.computer_id = a.computer_id AND e.engine_id = a.engine_id
                WHERE a.id = $1 AND a.enabled AND a.computer_id = 'local'
                  AND e.status = 'ready'
             )",
        )
        .bind(agent_id)
        .fetch_one(&self.pool)
        .await?;
        if !available {
            return Err(protocol_error("ENGINE_UNAVAILABLE: agent is not runnable"));
        }
        Ok(())
    }

    pub async fn authorize_agent(&self, claims: &AgentClaims) -> Result<(), sqlx::Error> {
        let valid: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM collab_agents a
                JOIN collab_computers c ON c.id = a.computer_id
                WHERE a.id = $1 AND a.enabled AND a.computer_id = 'local'
                  AND c.daemon_generation = $2 AND c.status = 'online'
             )",
        )
        .bind(&claims.sub)
        .bind(claims.generation)
        .fetch_one(&self.pool)
        .await?;
        if !valid {
            return Err(protocol_error(
                "FENCED: agent generation is no longer active",
            ));
        }
        Ok(())
    }

    pub async fn inbox(&self, claims: &AgentClaims) -> Result<InboxResponse, sqlx::Error> {
        let rows = sqlx::query_as::<_, InboxRow>(
            "SELECT m.id, m.room_id, m.sequence, m.author_id, m.body, rm.last_read_seq,
                    COUNT(*) OVER() AS total_count
             FROM collab_room_members rm
             JOIN collab_rooms r ON r.id = rm.room_id
             JOIN collab_messages m ON m.room_id = rm.room_id
             WHERE rm.participant_id = $1
               AND (
                   NOT rm.muted OR r.kind = 'direct' OR EXISTS (
                       SELECT 1
                       FROM collab_messages mention
                       WHERE mention.room_id = rm.room_id
                         AND mention.sequence > rm.last_read_seq
                         AND mention.author_id <> $1
                         AND mention.kind = 'normal'
                         AND mention.body ~ (
                             '(^|[^A-Za-z0-9_])@' || rm.participant_id ||
                             '([^A-Za-z0-9_]|$)'
                         )
                   )
               )
               AND m.sequence > rm.last_read_seq AND m.author_id <> $1
             ORDER BY m.created_at, m.room_id, m.sequence
             LIMIT 200",
        )
        .bind(&claims.sub)
        .fetch_all(&self.pool)
        .await?;
        if rows.is_empty() {
            return Ok(InboxResponse {
                trigger: None,
                messages: Vec::new(),
                carried_over: false,
            });
        }
        let carried_over = rows
            .first()
            .is_some_and(|row| row.total_count > rows.len() as i64);
        let mut ranges = BTreeMap::<String, (i64, i64)>::new();
        let mut messages = Vec::with_capacity(rows.len());
        for row in rows {
            ranges
                .entry(row.room_id.clone())
                .and_modify(|range| range.1 = row.sequence)
                .or_insert((row.last_read_seq + 1, row.sequence));
            messages.push(MessageView {
                id: row.id,
                room_id: row.room_id,
                sequence: row.sequence,
                author_id: row.author_id,
                body: row.body,
            });
        }
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        Ok(InboxResponse {
            trigger: Some(TriggerEnvelope {
                protocol_version: COLLAB_PROTOCOL_VERSION,
                dispatch_id: format!("run_{}", Uuid::new_v4().simple()),
                agent_id: claims.sub.clone(),
                computer_id: "local".to_string(),
                computer_generation: claims.generation,
                trigger: "message".to_string(),
                deliveries: ranges
                    .into_iter()
                    .map(|(room_id, (from_seq, up_to_seq))| DeliveryRange {
                        room_id,
                        from_seq,
                        up_to_seq,
                    })
                    .collect(),
                agenda_focus: None,
                carried_over,
                issued_at: now,
                expires_at: now + 5 * 60,
                signature: String::new(),
            }),
            messages,
            carried_over,
        })
    }

    pub async fn agent_triage_profile(
        &self,
        claims: &AgentClaims,
    ) -> Result<(String, Option<String>, Option<String>, String), sqlx::Error> {
        sqlx::query_as(
            "SELECT a.system_prompt, a.role, a.bio,
                    COALESCE(a.fast_model, a.model) AS fast_model
             FROM collab_agents a
             JOIN collab_computers c ON c.id = a.computer_id
             WHERE a.id = $1 AND a.enabled AND c.daemon_generation = $2
               AND c.status = 'online'",
        )
        .bind(&claims.sub)
        .bind(claims.generation)
        .fetch_one(&self.pool)
        .await
    }

    pub(super) async fn triage_context(
        &self,
        claims: &AgentClaims,
        run_id: &str,
    ) -> Result<TriageContext, sqlx::Error> {
        let unread = sqlx::query_as::<_, TriageMessage>(
            "SELECT m.id, m.room_id, room.kind AS room_kind, m.sequence,
                    m.author_id, author.kind AS author_kind,
                    author.display_name AS author_name, m.kind AS message_kind, m.body
             FROM collab_runs r
             JOIN collab_run_deliveries d ON d.run_id = r.id
             JOIN collab_messages m ON m.room_id = d.room_id
                AND m.sequence BETWEEN d.from_seq AND d.up_to_seq
             JOIN collab_rooms room ON room.id = m.room_id
             JOIN collab_participants author ON author.id = m.author_id
             WHERE r.id = $1 AND r.agent_id = $2
               AND r.computer_generation = $3 AND r.status = 'running'
               AND m.author_id <> $2
             ORDER BY m.room_id, m.sequence",
        )
        .bind(run_id)
        .bind(&claims.sub)
        .bind(claims.generation)
        .fetch_all(&self.pool)
        .await?;
        if unread.is_empty() {
            return Err(sqlx::Error::RowNotFound);
        }
        let recent = sqlx::query_as::<_, TriageMessage>(
            "SELECT message.id, message.room_id, room.kind AS room_kind,
                    message.sequence, message.author_id,
                    author.kind AS author_kind, author.display_name AS author_name,
                    message.kind AS message_kind, message.body
             FROM collab_runs run
             JOIN collab_run_deliveries delivery ON delivery.run_id = run.id
             JOIN collab_room_members member
               ON member.room_id = delivery.room_id
              AND member.participant_id = run.agent_id
             JOIN LATERAL (
                 SELECT history.id, history.room_id, history.sequence,
                        history.author_id, history.kind, history.body
                 FROM collab_messages history
                 WHERE history.room_id = delivery.room_id
                   AND history.sequence < delivery.from_seq
                   AND history.created_at >= member.joined_at
                 ORDER BY history.sequence DESC
                 LIMIT 12
             ) message ON TRUE
             JOIN collab_rooms room ON room.id = message.room_id
             JOIN collab_participants author ON author.id = message.author_id
             WHERE run.id = $1 AND run.agent_id = $2
               AND run.computer_generation = $3 AND run.status = 'running'
             ORDER BY message.room_id, message.sequence",
        )
        .bind(run_id)
        .bind(&claims.sub)
        .bind(claims.generation)
        .fetch_all(&self.pool)
        .await?;
        Ok(TriageContext { unread, recent })
    }

    pub async fn record_triage(
        &self,
        claims: &AgentClaims,
        request: &TriageReportRequest,
    ) -> Result<(), sqlx::Error> {
        let mut transaction = self.pool.begin().await?;
        self.authorize_agent_transaction(&mut transaction, claims)
            .await?;
        if !matches!(
            request.verdict.source.as_str(),
            "local_model" | "deterministic" | "system_only"
        ) {
            return Err(protocol_error("INVALID_ARGUMENT: invalid triage source"));
        }
        if request.verdict.source == "system_only" && request.verdict.actionable {
            return Err(protocol_error(
                "INVALID_ARGUMENT: system-only triage cannot be actionable",
            ));
        }
        let existing: Option<bool> = sqlx::query_scalar(
            "SELECT actionable FROM collab_triages
             WHERE run_id = $1
             LIMIT 1",
        )
        .bind(&request.run_id)
        .fetch_optional(&mut *transaction)
        .await?;
        if existing.is_some_and(|actionable| actionable != request.verdict.actionable) {
            return Err(protocol_error(
                "CONFLICT: triage verdict is immutable once recorded",
            ));
        }
        let deliveries: Vec<(String, i64)> = sqlx::query_as(
            "SELECT d.room_id, d.up_to_seq
             FROM collab_run_deliveries d
             JOIN collab_runs r ON r.id = d.run_id
             WHERE d.run_id = $1 AND r.agent_id = $2
               AND r.computer_generation = $3 AND r.status = 'running'
             FOR UPDATE OF d",
        )
        .bind(&request.run_id)
        .bind(&claims.sub)
        .bind(claims.generation)
        .fetch_all(&mut *transaction)
        .await?;
        if deliveries.is_empty() {
            return Err(sqlx::Error::RowNotFound);
        }
        for (room_id, up_to_seq) in deliveries {
            sqlx::query(
                "INSERT INTO collab_triages (
                    id, run_id, agent_id, computer_id, room_id, up_to_seq,
                    actionable, source, reason, prompt_note, engine_id, model,
                    input_tokens, output_tokens, latency_ms
                 )
                 SELECT $1, $2, $3, 'local', $4, $5, $6, $7,
                        $8, $9, 'opencode', $10, $11, $12, $13
                 WHERE NOT EXISTS (
                    SELECT 1 FROM collab_triages
                    WHERE run_id = $2 AND room_id = $4
                 )",
            )
            .bind(format!("triage_{}", Uuid::new_v4().simple()))
            .bind(&request.run_id)
            .bind(&claims.sub)
            .bind(&room_id)
            .bind(up_to_seq)
            .bind(request.verdict.actionable)
            .bind(&request.verdict.source)
            .bind(&request.verdict.reason)
            .bind(&request.verdict.prompt_note)
            .bind(&request.model)
            .bind(request.input_tokens)
            .bind(request.output_tokens)
            .bind(request.latency_ms)
            .execute(&mut *transaction)
            .await?;
            if !request.verdict.actionable {
                sqlx::query(
                    "UPDATE collab_run_deliveries
                     SET eligible_reason = 'triage_false',
                         eligible_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
                     WHERE run_id = $1 AND room_id = $2 AND eligible_reason IS NULL",
                )
                .bind(&request.run_id)
                .bind(&room_id)
                .execute(&mut *transaction)
                .await?;
            }
        }
        transaction.commit().await
    }

    pub async fn open_run(
        &self,
        claims: &AgentClaims,
        trigger: &TriggerEnvelope,
    ) -> Result<RunView, sqlx::Error> {
        let mut transaction = self.pool.begin().await?;
        self.authorize_agent_transaction(&mut transaction, claims)
            .await?;
        let focus = trigger.agenda_focus.as_ref();
        if let Some(focus) = focus {
            let current: bool = sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1
                    FROM collab_rooms room
                    JOIN collab_room_members member
                      ON member.room_id = room.id AND member.participant_id = $2
                    WHERE room.id = $1 AND room.next_seq = $3
                      AND (
                        $4::TEXT IS NULL OR EXISTS (
                            SELECT 1
                            FROM collab_cards card
                            JOIN collab_boards board ON board.id = card.board_id
                            JOIN collab_board_columns board_column
                              ON board_column.id = card.column_id AND NOT board_column.is_done
                            WHERE card.id = $4 AND board.room_id = room.id
                              AND (
                                card.claimed_by = $2 OR
                                (card.claimed_by IS NULL AND card.assignee_id = $2)
                              )
                        )
                      )
                 )",
            )
            .bind(&focus.room_id)
            .bind(&claims.sub)
            .bind(focus.room_sequence)
            .bind(&focus.card_id)
            .fetch_one(&mut *transaction)
            .await?;
            if !current {
                return Err(protocol_error(
                    "CONFLICT: agenda focus changed before the run opened",
                ));
            }
        }
        let room_id = focus.map(|focus| focus.room_id.as_str()).or_else(|| {
            trigger
                .deliveries
                .first()
                .map(|delivery| delivery.room_id.as_str())
        });
        sqlx::query(
            "INSERT INTO collab_runs (
                id, agent_id, computer_id, room_id, trigger, status,
                engine_id, model, computer_generation, inbox_carried_over,
                focus_card_id, agenda_anchor_seq, trigger_reason
             )
             SELECT $1, a.id, a.computer_id, $3, $4, 'running',
                    a.engine_id, a.model, $5, $6, $7, $8, $9
             FROM collab_agents a
             WHERE a.id = $2
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(&trigger.dispatch_id)
        .bind(&claims.sub)
        .bind(room_id)
        .bind(&trigger.trigger)
        .bind(claims.generation)
        .bind(trigger.carried_over)
        .bind(focus.and_then(|focus| focus.card_id.as_deref()))
        .bind(focus.map(|focus| focus.room_sequence))
        .bind(focus.map(|focus| focus.reason.as_str()))
        .execute(&mut *transaction)
        .await?;
        for delivery in &trigger.deliveries {
            let member: bool = sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1 FROM collab_room_members
                    WHERE room_id = $1 AND participant_id = $2
                 )",
            )
            .bind(&delivery.room_id)
            .bind(&claims.sub)
            .fetch_one(&mut *transaction)
            .await?;
            if !member {
                return Err(sqlx::Error::RowNotFound);
            }
            sqlx::query(
                "INSERT INTO collab_run_deliveries (run_id, room_id, from_seq, up_to_seq)
                 VALUES ($1, $2, $3, $4)
                 ON CONFLICT (run_id, room_id) DO NOTHING",
            )
            .bind(&trigger.dispatch_id)
            .bind(&delivery.room_id)
            .bind(delivery.from_seq)
            .bind(delivery.up_to_seq)
            .execute(&mut *transaction)
            .await?;
        }
        let status: String =
            sqlx::query_scalar("SELECT status FROM collab_runs WHERE id = $1 AND agent_id = $2")
                .bind(&trigger.dispatch_id)
                .bind(&claims.sub)
                .fetch_one(&mut *transaction)
                .await?;
        transaction.commit().await?;
        Ok(RunView {
            id: trigger.dispatch_id.clone(),
            status,
            outcome: None,
        })
    }

    pub async fn heartbeat_run(
        &self,
        claims: &AgentClaims,
        run_id: &str,
    ) -> Result<(), sqlx::Error> {
        let mut transaction = self.pool.begin().await?;
        self.authorize_agent_transaction(&mut transaction, claims)
            .await?;
        let result = sqlx::query(
            "UPDATE collab_runs
             SET heartbeat_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             WHERE id = $1 AND agent_id = $2 AND computer_generation = $3
               AND status = 'running'",
        )
        .bind(run_id)
        .bind(&claims.sub)
        .bind(claims.generation)
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() == 0 {
            return Err(sqlx::Error::RowNotFound);
        }
        transaction.commit().await
    }

    pub async fn finish_run(
        &self,
        claims: &AgentClaims,
        run_id: &str,
        request: FinishRunRequest,
    ) -> Result<RunView, sqlx::Error> {
        let terminal_status = match request.status.as_str() {
            "completed" | "failed" | "cancelled" | "interrupted" => request.status.as_str(),
            _ => return Err(protocol_error("INVALID_ARGUMENT: invalid run status")),
        };
        let mut transaction = self.pool.begin().await?;
        self.authorize_agent_transaction(&mut transaction, claims)
            .await?;
        let (current_status, current_outcome): (String, Option<String>) = sqlx::query_as(
            "SELECT status, outcome FROM collab_runs
             WHERE id = $1 AND agent_id = $2 AND computer_generation = $3
             FOR UPDATE",
        )
        .bind(run_id)
        .bind(&claims.sub)
        .bind(claims.generation)
        .fetch_one(&mut *transaction)
        .await?;
        if current_status != "running" {
            transaction.commit().await?;
            return Ok(RunView {
                id: run_id.to_string(),
                status: current_status,
                outcome: current_outcome,
            });
        }
        let mut outcome = None;
        if terminal_status == "completed" {
            let deliveries: Vec<(String, i64, Option<String>)> = sqlx::query_as(
                "SELECT room_id, up_to_seq, eligible_reason
                 FROM collab_run_deliveries WHERE run_id = $1",
            )
            .bind(run_id)
            .fetch_all(&mut *transaction)
            .await?;
            let mut acted = false;
            let mut acknowledged = false;
            for (room_id, up_to_seq, reason) in deliveries {
                if let Some(reason) = reason {
                    sqlx::query(
                        "UPDATE collab_room_members
                         SET last_read_seq = GREATEST(last_read_seq, $1)
                         WHERE room_id = $2 AND participant_id = $3",
                    )
                    .bind(up_to_seq)
                    .bind(&room_id)
                    .bind(&claims.sub)
                    .execute(&mut *transaction)
                    .await?;
                    sqlx::query(
                        "UPDATE collab_run_deliveries
                         SET settled_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
                         WHERE run_id = $1 AND room_id = $2",
                    )
                    .bind(run_id)
                    .bind(&room_id)
                    .execute(&mut *transaction)
                    .await?;
                    acted |= reason == "action";
                    acknowledged |= reason == "ack" || reason == "triage_false";
                }
            }
            let action_recorded: bool = sqlx::query_scalar(
                "SELECT EXISTS (
                    SELECT 1
                    FROM collab_cli_requests request,
                         LATERAL jsonb_array_elements(
                             COALESCE(request.result -> 'sideEffects', '[]'::jsonb)
                         ) effect
                    WHERE request.run_id = $1
                      AND effect ->> 'type' IN (
                          'message_published', 'reaction_changed',
                          'card_created', 'card_claimed', 'card_moved'
                      )
                 )",
            )
            .bind(run_id)
            .fetch_one(&mut *transaction)
            .await?;
            acted |= action_recorded;
            outcome = Some(if acted {
                "acted"
            } else if acknowledged
                || request
                    .assistant_text
                    .as_deref()
                    .is_none_or(|text| text.trim().is_empty())
            {
                "silent"
            } else {
                "unpublished"
            });
        }
        sqlx::query(
            "UPDATE collab_runs
             SET status = $1, ended_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
                 heartbeat_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
                 input_tokens = $2, cached_input_tokens = $3, output_tokens = $4,
                 error_code = $5, error_message = $6, outcome = $7
             WHERE id = $8",
        )
        .bind(terminal_status)
        .bind(request.input_tokens)
        .bind(request.cached_input_tokens)
        .bind(request.output_tokens)
        .bind(request.error_code)
        .bind(request.error_message)
        .bind(outcome)
        .bind(run_id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(RunView {
            id: run_id.to_string(),
            status: terminal_status.to_string(),
            outcome: outcome.map(str::to_string),
        })
    }

    async fn authorize_device(
        &self,
        device_token: &str,
        generation: i64,
    ) -> Result<(), sqlx::Error> {
        let row: (String, i64) = sqlx::query_as(
            "SELECT credential_hash, daemon_generation
             FROM collab_computers WHERE id = 'local'",
        )
        .fetch_one(&self.pool)
        .await?;
        authenticate_hash(device_token, &row.0)?;
        if row.1 != generation {
            return Err(protocol_error("FENCED: stale daemon generation"));
        }
        Ok(())
    }

    async fn authorize_device_transaction(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        device_token: &str,
        generation: i64,
    ) -> Result<(), sqlx::Error> {
        let row: (String, i64) = sqlx::query_as(
            "SELECT credential_hash, daemon_generation
             FROM collab_computers WHERE id = 'local'
             FOR SHARE",
        )
        .fetch_one(&mut **transaction)
        .await?;
        authenticate_hash(device_token, &row.0)?;
        if row.1 != generation {
            return Err(protocol_error("FENCED: stale daemon generation"));
        }
        Ok(())
    }

    async fn authorize_agent_transaction(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
        claims: &AgentClaims,
    ) -> Result<(), sqlx::Error> {
        let generation: Option<i64> = sqlx::query_scalar(
            "SELECT c.daemon_generation
             FROM collab_agents a
             JOIN collab_computers c ON c.id = a.computer_id
             WHERE a.id = $1 AND a.enabled AND a.computer_id = 'local'
               AND c.status = 'online'
             FOR SHARE OF c",
        )
        .bind(&claims.sub)
        .fetch_optional(&mut **transaction)
        .await?;
        if generation != Some(claims.generation) {
            return Err(protocol_error(
                "FENCED: agent generation is no longer active",
            ));
        }
        Ok(())
    }

    async fn insert_message(
        &self,
        room_id: &str,
        author_id: &str,
        body: &str,
    ) -> Result<MessageView, sqlx::Error> {
        let message_id = format!("msg_{}", Uuid::new_v4().simple());
        let mut transaction = self.pool.begin().await?;
        let sequence: i64 = sqlx::query_scalar(
            "UPDATE collab_rooms
             SET next_seq = next_seq + 1,
                 last_message_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             WHERE id = $1
             RETURNING next_seq",
        )
        .bind(room_id)
        .fetch_one(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO collab_messages (id, room_id, sequence, author_id, body)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(&message_id)
        .bind(room_id)
        .bind(sequence)
        .bind(author_id)
        .bind(body)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(MessageView {
            id: message_id,
            room_id: room_id.to_string(),
            sequence,
            author_id: author_id.to_string(),
            body: body.to_string(),
        })
    }
}

#[derive(FromRow)]
struct ComputerRow {
    id: String,
    name: String,
    status: String,
    daemon_generation: i64,
    engine_id: String,
    engine_status: String,
}

#[derive(FromRow)]
struct MessageRow {
    id: String,
    room_id: String,
    sequence: i64,
    author_id: String,
    body: String,
}

pub(super) struct TriageContext {
    pub unread: Vec<TriageMessage>,
    pub recent: Vec<TriageMessage>,
}

#[derive(FromRow)]
pub(super) struct TriageMessage {
    pub id: String,
    pub room_id: String,
    pub room_kind: String,
    pub sequence: i64,
    pub author_id: String,
    pub author_kind: String,
    pub author_name: String,
    pub message_kind: String,
    pub body: String,
}

#[derive(FromRow)]
struct AssignmentRow {
    id: String,
    display_name: String,
    role: Option<String>,
    bio: Option<String>,
    system_prompt: String,
    engine_id: String,
    model: String,
    fast_model: String,
    config_version: i64,
    scanner_enabled: bool,
}

#[derive(FromRow)]
struct AgentViewRow {
    id: String,
    display_name: String,
    system_prompt: String,
    engine_id: String,
    model: String,
    config_version: i64,
    enabled: bool,
    scanner_enabled: bool,
}

impl From<AgentViewRow> for AgentView {
    fn from(row: AgentViewRow) -> Self {
        Self {
            id: row.id,
            display_name: row.display_name,
            system_prompt: row.system_prompt,
            engine_id: row.engine_id,
            model: row.model,
            config_version: row.config_version,
            enabled: row.enabled,
            scanner_enabled: row.scanner_enabled,
        }
    }
}

#[derive(FromRow)]
struct BoardColumnRow {
    id: String,
    title: String,
    position: i32,
    is_done: bool,
}

#[derive(FromRow)]
struct CardRow {
    id: String,
    title: String,
    description: Option<String>,
    position: i32,
    assignee_id: Option<String>,
    claimed_by: Option<String>,
}

impl From<CardRow> for CardView {
    fn from(row: CardRow) -> Self {
        Self {
            id: row.id,
            title: row.title,
            description: row.description,
            position: row.position,
            assignee_id: row.assignee_id,
            claimed_by: row.claimed_by,
        }
    }
}

#[derive(FromRow)]
struct RunSummaryRow {
    id: String,
    agent_id: String,
    trigger: String,
    status: String,
    outcome: Option<String>,
    room_id: Option<String>,
    focus_card_id: Option<String>,
    trigger_reason: Option<String>,
    started_at: String,
}

impl From<RunSummaryRow> for RunSummaryView {
    fn from(row: RunSummaryRow) -> Self {
        Self {
            id: row.id,
            agent_id: row.agent_id,
            trigger: row.trigger,
            status: row.status,
            outcome: row.outcome,
            room_id: row.room_id,
            focus_card_id: row.focus_card_id,
            trigger_reason: row.trigger_reason,
            started_at: row.started_at,
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

impl From<AssignmentRow> for AgentAssignment {
    fn from(row: AssignmentRow) -> Self {
        Self {
            id: row.id,
            display_name: row.display_name,
            role: row.role,
            bio: row.bio,
            system_prompt: row.system_prompt,
            engine_id: row.engine_id,
            model: row.model,
            fast_model: row.fast_model,
            config_version: row.config_version,
            scanner_enabled: row.scanner_enabled,
        }
    }
}

#[derive(FromRow)]
struct InboxRow {
    id: String,
    room_id: String,
    sequence: i64,
    author_id: String,
    body: String,
    last_read_seq: i64,
    total_count: i64,
}

impl From<MessageRow> for MessageView {
    fn from(row: MessageRow) -> Self {
        Self {
            id: row.id,
            room_id: row.room_id,
            sequence: row.sequence,
            author_id: row.author_id,
            body: row.body,
        }
    }
}

fn device_token() -> String {
    format!(
        "owd_{}_{}_{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    )
}

fn token_hash(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    let mut encoded = String::with_capacity(7 + digest.len() * 2);
    encoded.push_str("sha256:");
    for byte in digest {
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

fn authenticate_hash(token: &str, expected: &str) -> Result<(), sqlx::Error> {
    let actual = token_hash(token);
    let same = actual.len() == expected.len()
        && actual
            .bytes()
            .zip(expected.bytes())
            .fold(0_u8, |difference, (left, right)| {
                difference | (left ^ right)
            })
            == 0;
    if same {
        Ok(())
    } else {
        Err(protocol_error("UNAUTHENTICATED: invalid device token"))
    }
}

fn protocol_error(message: &str) -> sqlx::Error {
    sqlx::Error::Protocol(message.to_string())
}

fn engine_status_name(status: EngineStatus) -> &'static str {
    match status {
        EngineStatus::Unknown => "unknown",
        EngineStatus::Ready => "ready",
        EngineStatus::Missing => "missing",
    }
}

fn parse_computer_status(value: &str) -> ComputerStatus {
    match value {
        "online" => ComputerStatus::Online,
        _ => ComputerStatus::Offline,
    }
}

fn parse_engine_status(value: &str) -> EngineStatus {
    match value {
        "ready" => EngineStatus::Ready,
        "missing" => EngineStatus::Missing,
        _ => EngineStatus::Unknown,
    }
}
