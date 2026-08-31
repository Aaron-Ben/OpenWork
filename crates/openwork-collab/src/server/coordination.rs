use std::{fmt::Write as _, time::Duration};

use redis::{ErrorKind, RedisError, RedisResult, Script};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::redis::RedisCoordination;

const SEEN_TTL_SECONDS: usize = 10 * 60;
const HELD_TTL_SECONDS: usize = 2 * 60;
const AGENDA_RATE_SECONDS: usize = 5 * 60;
const AGENDA_NUDGE_SECONDS: usize = 45 * 60;
const AGENDA_DECLINE_SECONDS: usize = 6 * 60 * 60;
const AGENDA_DECLINE_CAP: i64 = 3;
const REDIS_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub struct Coordination {
    redis: RedisCoordination,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HeldBinding {
    pub agent_id: String,
    pub run_id: String,
    pub room_id: String,
    pub runtime_session_id: String,
    pub shown_peer_max: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HeldReservation {
    Reserved(HeldBinding),
    Missing,
    OwnedByAnotherRequest,
}

impl Coordination {
    pub fn new(redis: RedisCoordination) -> Self {
        Self { redis }
    }

    pub async fn record_seen(
        &self,
        agent_id: &str,
        room_id: &str,
        sequence: i64,
    ) -> RedisResult<i64> {
        let mut connection = self.redis.connection().await?;
        let key = format!("openwork:seen:{agent_id}:{room_id}");
        tokio::time::timeout(
            REDIS_TIMEOUT,
            Script::new(
                r#"
                local current = tonumber(redis.call('GET', KEYS[1]) or '-1')
                local incoming = tonumber(ARGV[1])
                if incoming > current then
                    redis.call('SET', KEYS[1], incoming, 'EX', ARGV[2])
                    return incoming
                end
                redis.call('EXPIRE', KEYS[1], ARGV[2])
                return current
                "#,
            )
            .key(key)
            .arg(sequence)
            .arg(SEEN_TTL_SECONDS)
            .invoke_async(&mut connection),
        )
        .await
        .map_err(|_| timeout_error())?
    }

    pub async fn get_seen(&self, agent_id: &str, room_id: &str) -> RedisResult<Option<i64>> {
        let mut connection = self.redis.connection().await?;
        let key = format!("openwork:seen:{agent_id}:{room_id}");
        tokio::time::timeout(
            REDIS_TIMEOUT,
            redis::cmd("GET").arg(key).query_async(&mut connection),
        )
        .await
        .map_err(|_| timeout_error())?
    }

    pub async fn issue_held(&self, binding: &HeldBinding) -> RedisResult<String> {
        let token = format!(
            "hold_{}_{}_{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        );
        let key = held_key(&binding.agent_id, &binding.room_id, &token);
        let value = serde_json::to_string(binding).map_err(|error| {
            RedisError::from((
                ErrorKind::UnexpectedReturnType,
                "invalid HELD binding",
                error.to_string(),
            ))
        })?;
        let mut connection = self.redis.connection().await?;
        let stored = tokio::time::timeout(
            REDIS_TIMEOUT,
            redis::cmd("SET")
                .arg(key)
                .arg(value)
                .arg("NX")
                .arg("EX")
                .arg(HELD_TTL_SECONDS)
                .query_async::<Option<String>>(&mut connection),
        )
        .await
        .map_err(|_| timeout_error())??;
        stored
            .is_some()
            .then_some(token)
            .ok_or_else(|| RedisError::from((ErrorKind::Client, "HELD token collision")))
    }

    pub async fn reserve_held(
        &self,
        agent_id: &str,
        room_id: &str,
        token: &str,
        request_id: &str,
    ) -> RedisResult<HeldReservation> {
        let mut connection = self.redis.connection().await?;
        let key = held_key(agent_id, room_id, token);
        let (status, value): (String, String) = tokio::time::timeout(
            REDIS_TIMEOUT,
            Script::new(
                r#"
                local binding = redis.call('GET', KEYS[1])
                if not binding then
                    return {'missing', ''}
                end
                local owner = redis.call('GET', KEYS[2])
                if not owner then
                    redis.call('SET', KEYS[2], ARGV[1], 'NX', 'EX', ARGV[2])
                    owner = redis.call('GET', KEYS[2])
                end
                if owner == ARGV[1] then
                    return {'reserved', binding}
                end
                return {'owned', ''}
                "#,
            )
            .key(&key)
            .key(held_reservation_key(&key))
            .arg(request_id)
            .arg(HELD_TTL_SECONDS)
            .invoke_async(&mut connection),
        )
        .await
        .map_err(|_| timeout_error())??;
        match status.as_str() {
            "missing" => Ok(HeldReservation::Missing),
            "owned" => Ok(HeldReservation::OwnedByAnotherRequest),
            "reserved" => serde_json::from_str(&value)
                .map(HeldReservation::Reserved)
                .map_err(|error| {
                    RedisError::from((
                        ErrorKind::UnexpectedReturnType,
                        "invalid stored HELD binding",
                        error.to_string(),
                    ))
                }),
            _ => Err(RedisError::from((
                ErrorKind::UnexpectedReturnType,
                "invalid HELD reservation response",
                status,
            ))),
        }
    }

    pub async fn finalize_held(
        &self,
        agent_id: &str,
        room_id: &str,
        token: &str,
        request_id: &str,
    ) -> RedisResult<bool> {
        let mut connection = self.redis.connection().await?;
        let key = held_key(agent_id, room_id, token);
        let deleted: i64 = tokio::time::timeout(
            REDIS_TIMEOUT,
            Script::new(
                r#"
                if redis.call('GET', KEYS[2]) ~= ARGV[1] then
                    return 0
                end
                redis.call('DEL', KEYS[1], KEYS[2])
                return 1
                "#,
            )
            .key(&key)
            .key(held_reservation_key(&key))
            .arg(request_id)
            .invoke_async(&mut connection),
        )
        .await
        .map_err(|_| timeout_error())??;
        Ok(deleted == 1)
    }

    pub async fn agenda_allowed(&self, agent_id: &str) -> RedisResult<bool> {
        let mut connection = self.redis.connection().await?;
        let count: Option<i64> = tokio::time::timeout(
            REDIS_TIMEOUT,
            redis::cmd("GET")
                .arg(format!("openwork:agenda-declines:{agent_id}"))
                .query_async(&mut connection),
        )
        .await
        .map_err(|_| timeout_error())??;
        Ok(count.unwrap_or(0) < AGENDA_DECLINE_CAP)
    }

    pub async fn record_agenda_decline(&self, agent_id: &str) -> RedisResult<i64> {
        let mut connection = self.redis.connection().await?;
        let key = format!("openwork:agenda-declines:{agent_id}");
        tokio::time::timeout(
            REDIS_TIMEOUT,
            Script::new(
                r#"
                local count = redis.call('INCR', KEYS[1])
                redis.call('EXPIRE', KEYS[1], ARGV[1])
                return count
                "#,
            )
            .key(key)
            .arg(AGENDA_DECLINE_SECONDS)
            .invoke_async(&mut connection),
        )
        .await
        .map_err(|_| timeout_error())?
    }

    pub async fn reset_agenda_declines(&self, agent_ids: &[String]) -> RedisResult<()> {
        if agent_ids.is_empty() {
            return Ok(());
        }
        let mut connection = self.redis.connection().await?;
        let mut command = redis::cmd("DEL");
        for agent_id in agent_ids {
            command.arg(format!("openwork:agenda-declines:{agent_id}"));
        }
        tokio::time::timeout(REDIS_TIMEOUT, command.query_async::<i64>(&mut connection))
            .await
            .map_err(|_| timeout_error())??;
        Ok(())
    }

    pub async fn claim_agenda_rate(&self, agent_id: &str) -> RedisResult<bool> {
        self.claim_once(
            format!("openwork:agenda-rate:{agent_id}"),
            AGENDA_RATE_SECONDS,
        )
        .await
    }

    pub async fn claim_room_nudge(&self, room_id: &str) -> RedisResult<bool> {
        self.claim_once(
            format!("openwork:agenda-nudge:{room_id}"),
            AGENDA_NUDGE_SECONDS,
        )
        .await
    }

    async fn claim_once(&self, key: String, ttl_seconds: usize) -> RedisResult<bool> {
        let mut connection = self.redis.connection().await?;
        let stored = tokio::time::timeout(
            REDIS_TIMEOUT,
            redis::cmd("SET")
                .arg(key)
                .arg("1")
                .arg("NX")
                .arg("EX")
                .arg(ttl_seconds)
                .query_async::<Option<String>>(&mut connection),
        )
        .await
        .map_err(|_| timeout_error())??;
        Ok(stored.is_some())
    }
}

fn held_key(agent_id: &str, room_id: &str, token: &str) -> String {
    format!(
        "openwork:hold:{agent_id}:{room_id}:{}",
        digest(token.as_bytes())
    )
}

fn held_reservation_key(held_key: &str) -> String {
    format!("{held_key}:request")
}

fn digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

fn timeout_error() -> RedisError {
    RedisError::from((ErrorKind::Io, "Redis operation timed out"))
}

#[cfg(test)]
mod tests {
    use tokio_util::sync::CancellationToken;

    use super::*;

    #[tokio::test]
    async fn held_token_reservation_is_scoped_to_one_request_and_retriable() {
        let Ok(redis_url) = std::env::var("TEST_REDIS_URL").or_else(|_| std::env::var("REDIS_URL"))
        else {
            return;
        };
        let shutdown = CancellationToken::new();
        let Ok((redis, subscriber)) =
            RedisCoordination::start(&redis_url, shutdown.child_token()).await
        else {
            return;
        };
        if !redis.is_connected() {
            shutdown.cancel();
            let _ = subscriber.await;
            return;
        }
        let coordination = Coordination::new(redis);
        let suffix = Uuid::new_v4().simple().to_string();
        let binding = HeldBinding {
            agent_id: format!("agent_{suffix}"),
            run_id: format!("run_{suffix}"),
            room_id: format!("room_{suffix}"),
            runtime_session_id: format!("runtime_{suffix}"),
            shown_peer_max: 42,
        };
        let token = coordination.issue_held(&binding).await.unwrap();

        assert_eq!(
            coordination
                .reserve_held(&binding.agent_id, &binding.room_id, &token, "request-a")
                .await
                .unwrap(),
            HeldReservation::Reserved(binding.clone())
        );
        assert_eq!(
            coordination
                .reserve_held(&binding.agent_id, &binding.room_id, &token, "request-a")
                .await
                .unwrap(),
            HeldReservation::Reserved(binding.clone())
        );
        assert_eq!(
            coordination
                .reserve_held(&binding.agent_id, &binding.room_id, &token, "request-b")
                .await
                .unwrap(),
            HeldReservation::OwnedByAnotherRequest
        );
        assert!(
            !coordination
                .finalize_held(&binding.agent_id, &binding.room_id, &token, "request-b")
                .await
                .unwrap()
        );
        assert!(
            coordination
                .finalize_held(&binding.agent_id, &binding.room_id, &token, "request-a")
                .await
                .unwrap()
        );
        assert_eq!(
            coordination
                .reserve_held(&binding.agent_id, &binding.room_id, &token, "request-a")
                .await
                .unwrap(),
            HeldReservation::Missing
        );

        shutdown.cancel();
        let _ = subscriber.await;
    }
}
