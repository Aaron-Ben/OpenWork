use std::{fmt::Write as _, time::Duration};

use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::protocol::{
    COLLAB_PROTOCOL_VERSION, ComputerStatus, ComputerView, EngineStatus, HeartbeatRequest,
    LocalComputerRegistration,
};

use super::auth::AgentClaims;

#[derive(Clone)]
pub(crate) struct Computers {
    pool: PgPool,
}

impl Computers {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) async fn ensure_local(
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

    pub(crate) async fn start(&self, device_token: &str) -> Result<i64, sqlx::Error> {
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

    pub(crate) async fn heartbeat(
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
        authorize_device_transaction(&mut transaction, device_token, heartbeat.generation).await?;
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

    pub(crate) async fn sweep_offline(&self, lease: Duration) -> Result<bool, sqlx::Error> {
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

    pub(crate) async fn authorize_device(
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

    pub(crate) async fn authorize_agent_token(
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

    pub(crate) async fn authorize_agent(&self, claims: &AgentClaims) -> Result<(), sqlx::Error> {
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
}

pub(crate) async fn authorize_agent_transaction(
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

async fn authorize_device_transaction(
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

#[derive(FromRow)]
struct ComputerRow {
    id: String,
    name: String,
    status: String,
    daemon_generation: i64,
    engine_id: String,
    engine_status: String,
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

fn protocol_error(message: &str) -> sqlx::Error {
    sqlx::Error::Protocol(message.to_string())
}
