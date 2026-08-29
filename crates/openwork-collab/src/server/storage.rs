use std::{collections::BTreeMap, fmt::Write as _};

use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::protocol::{
    AgentAssignment, AgentView, COLLAB_PROTOCOL_VERSION, CliResult, CliSideEffect, ComputerStatus,
    ComputerView, DeliveryRange, EngineStatus, FinishRunRequest, HeartbeatRequest, InboxResponse,
    LocalComputerRegistration, MessageView, RoomView, RunView, TriggerEnvelope,
};

use super::auth::AgentClaims;

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
                id, computer_id, system_prompt, engine_id
             ) VALUES ($1, 'local', $2, 'opencode')",
        )
        .bind(id)
        .bind(system_prompt)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(AgentView {
            id: id.to_string(),
            display_name: display_name.to_string(),
            system_prompt: system_prompt.to_string(),
            engine_id: "opencode".to_string(),
            model: None,
            config_version: 1,
            enabled: true,
        })
    }

    pub async fn create_direct_room(&self, agent_id: &str) -> Result<RoomView, sqlx::Error> {
        let room_id = format!("room_{}", Uuid::new_v4().simple());
        let direct_key = if agent_id < "user" {
            format!("{agent_id}|user")
        } else {
            format!("user|{agent_id}")
        };
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO collab_rooms (id, kind, direct_key)
             VALUES ($1, 'direct', $2)",
        )
        .bind(&room_id)
        .bind(direct_key)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO collab_room_members (room_id, participant_id)
             VALUES ($1, 'user'), ($1, $2)",
        )
        .bind(&room_id)
        .bind(agent_id)
        .execute(&mut *transaction)
        .await?;
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
                    a.model, a.config_version, a.enabled
             FROM collab_agents a
             JOIN collab_participants p ON p.id = a.id
             ORDER BY a.id",
        )
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(AgentView::from).collect())
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
        let status = engine_status_name(heartbeat.engine.status);
        let checked = status != "unknown";
        let mut transaction = self.pool.begin().await?;
        self.authorize_device_transaction(&mut transaction, device_token, heartbeat.generation)
            .await?;
        sqlx::query(
            "UPDATE collab_computers
             SET status = 'online',
                 last_seen_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
                 daemon_version = $1, daemon_supervised = $2,
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             WHERE id = 'local' AND daemon_generation = $3",
        )
        .bind(&heartbeat.daemon_version)
        .bind(heartbeat.supervised)
        .bind(heartbeat.generation)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "UPDATE collab_computer_engines
             SET probe_version = probe_version + CASE
                    WHEN status IS DISTINCT FROM $1 OR version IS DISTINCT FROM $2 THEN 1 ELSE 0 END,
                 status = $1, version = $2,
                 checked_at = CASE WHEN $3 THEN CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
                                   ELSE NULL END
             WHERE computer_id = 'local' AND engine_id = 'opencode'",
        )
        .bind(status)
        .bind(&heartbeat.engine.version)
        .bind(checked)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await
    }

    pub async fn roster(
        &self,
        device_token: &str,
        generation: i64,
    ) -> Result<Vec<AgentAssignment>, sqlx::Error> {
        self.authorize_device(device_token, generation).await?;
        sqlx::query_as::<_, AssignmentRow>(
            "SELECT a.id, p.display_name, a.role, a.bio, a.system_prompt,
                    a.engine_id, a.model, a.config_version
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
            "SELECT m.id, m.room_id, m.sequence, m.author_id, m.body, rm.last_read_seq
             FROM collab_room_members rm
             JOIN collab_messages m ON m.room_id = rm.room_id
             WHERE rm.participant_id = $1 AND NOT rm.muted
               AND m.sequence > rm.last_read_seq AND m.author_id <> $1
             ORDER BY m.room_id, m.sequence
             LIMIT 200",
        )
        .bind(&claims.sub)
        .fetch_all(&self.pool)
        .await?;
        if rows.is_empty() {
            return Ok(InboxResponse {
                trigger: None,
                messages: Vec::new(),
            });
        }
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
                issued_at: now,
                expires_at: now + 5 * 60,
                signature: String::new(),
            }),
            messages,
        })
    }

    pub async fn open_run(
        &self,
        claims: &AgentClaims,
        trigger: &TriggerEnvelope,
    ) -> Result<RunView, sqlx::Error> {
        let mut transaction = self.pool.begin().await?;
        self.authorize_agent_transaction(&mut transaction, claims)
            .await?;
        sqlx::query(
            "INSERT INTO collab_runs (
                id, agent_id, computer_id, room_id, trigger, status,
                engine_id, computer_generation
             ) VALUES ($1, $2, 'local', $3, $4, 'running', 'opencode', $5)
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(&trigger.dispatch_id)
        .bind(&claims.sub)
        .bind(trigger.deliveries.first().map(|delivery| &delivery.room_id))
        .bind(&trigger.trigger)
        .bind(claims.generation)
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
        })
    }

    pub async fn run_cli(
        &self,
        claims: &AgentClaims,
        argv: Vec<String>,
    ) -> Result<CliResult, sqlx::Error> {
        if contains_identity_flag(&argv) {
            return Ok(cli_error(
                2,
                "INVALID_ARGUMENT: identity flags are not accepted",
            ));
        }
        let run_id: Option<String> = sqlx::query_scalar(
            "SELECT id FROM collab_runs
             WHERE agent_id = $1 AND computer_generation = $2 AND status = 'running'",
        )
        .bind(&claims.sub)
        .bind(claims.generation)
        .fetch_optional(&self.pool)
        .await?;
        let Some(run_id) = run_id else {
            return Ok(cli_error(3, "UNAUTHENTICATED: no active run"));
        };
        match argv.as_slice() {
            [command, room_id, separator, body]
                if command == "reply" && separator == "--" && !body.trim().is_empty() =>
            {
                self.reply(&run_id, claims, room_id, body).await
            }
            [command, room_id] if command == "ack" => self.ack(&run_id, claims, room_id).await,
            _ => Ok(cli_error(
                2,
                "INVALID_ARGUMENT: expected reply <room-id> -- <body> or ack <room-id>",
            )),
        }
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
        let current_status: String = sqlx::query_scalar(
            "SELECT status FROM collab_runs
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
                    acknowledged |= reason == "ack";
                }
            }
            outcome = Some(if acted {
                "acted"
            } else if acknowledged {
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

    async fn reply(
        &self,
        run_id: &str,
        claims: &AgentClaims,
        room_id: &str,
        body: &str,
    ) -> Result<CliResult, sqlx::Error> {
        let mut transaction = self.pool.begin().await?;
        self.authorize_agent_transaction(&mut transaction, claims)
            .await?;
        let row: Option<(i64, i64)> = sqlx::query_as(
            "SELECT d.up_to_seq, r.next_seq
             FROM collab_run_deliveries d
             JOIN collab_rooms r ON r.id = d.room_id
             JOIN collab_room_members m ON m.room_id = d.room_id AND m.participant_id = $3
             WHERE d.run_id = $1 AND d.room_id = $2
             FOR UPDATE OF r",
        )
        .bind(run_id)
        .bind(room_id)
        .bind(&claims.sub)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some((up_to_seq, current_seq)) = row else {
            return Ok(cli_error(4, "NOT_FOUND: room is not in the active run"));
        };
        if current_seq > up_to_seq {
            return Ok(cli_error(10, "HELD: room changed after this run began"));
        }
        let message_id = format!("msg_{}", Uuid::new_v4().simple());
        let sequence = current_seq + 1;
        sqlx::query(
            "UPDATE collab_rooms SET next_seq = $1,
                 last_message_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai',
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             WHERE id = $2",
        )
        .bind(sequence)
        .bind(room_id)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO collab_messages (id, room_id, sequence, author_id, body)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(&message_id)
        .bind(room_id)
        .bind(sequence)
        .bind(&claims.sub)
        .bind(body)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "UPDATE collab_run_deliveries
             SET eligible_reason = 'action',
                 eligible_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             WHERE run_id = $1 AND room_id = $2",
        )
        .bind(run_id)
        .bind(room_id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(CliResult {
            text: format!("Published message {message_id}"),
            exit_code: 0,
            side_effects: vec![CliSideEffect::MessagePublished {
                room_id: room_id.to_string(),
                message_id,
                sequence,
            }],
        })
    }

    async fn ack(
        &self,
        run_id: &str,
        claims: &AgentClaims,
        room_id: &str,
    ) -> Result<CliResult, sqlx::Error> {
        let mut transaction = self.pool.begin().await?;
        self.authorize_agent_transaction(&mut transaction, claims)
            .await?;
        let up_to_seq: Option<i64> = sqlx::query_scalar(
            "UPDATE collab_run_deliveries
             SET eligible_reason = 'ack',
                 eligible_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             WHERE run_id = $1 AND room_id = $2
             RETURNING up_to_seq",
        )
        .bind(run_id)
        .bind(room_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(up_to_seq) = up_to_seq else {
            return Ok(cli_error(4, "NOT_FOUND: room is not in the active run"));
        };
        transaction.commit().await?;
        Ok(CliResult {
            text: "Acknowledged".to_string(),
            exit_code: 0,
            side_effects: vec![CliSideEffect::InboxAcknowledged {
                room_id: room_id.to_string(),
                up_to_seq,
            }],
        })
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

#[derive(FromRow)]
struct AssignmentRow {
    id: String,
    display_name: String,
    role: Option<String>,
    bio: Option<String>,
    system_prompt: String,
    engine_id: String,
    model: Option<String>,
    config_version: i64,
}

#[derive(FromRow)]
struct AgentViewRow {
    id: String,
    display_name: String,
    system_prompt: String,
    engine_id: String,
    model: Option<String>,
    config_version: i64,
    enabled: bool,
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
            config_version: row.config_version,
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
        EngineStatus::Unauthenticated => "unauthenticated",
        EngineStatus::Broken => "broken",
    }
}

fn contains_identity_flag(argv: &[String]) -> bool {
    argv.iter()
        .take_while(|argument| argument.as_str() != "--")
        .any(|argument| {
            matches!(
                argument.as_str(),
                "--as" | "--agent" | "--agent-id" | "--run-id" | "--computer-id"
            ) || [
                "--as=",
                "--agent=",
                "--agent-id=",
                "--run-id=",
                "--computer-id=",
            ]
            .iter()
            .any(|prefix| argument.starts_with(prefix))
        })
}

fn cli_error(exit_code: i32, message: &str) -> CliResult {
    CliResult {
        text: message.to_string(),
        exit_code,
        side_effects: Vec::new(),
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
        "unauthenticated" => EngineStatus::Unauthenticated,
        "broken" => EngineStatus::Broken,
        _ => EngineStatus::Unknown,
    }
}
