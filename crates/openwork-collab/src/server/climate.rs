use sqlx::{FromRow, PgPool, Postgres, Transaction};

use crate::protocol::ClimateView;

pub(crate) const CLIMATE_NOTE_MAX_BYTES: usize = 4_000;

pub(crate) struct Climate;

impl Climate {
    pub(crate) async fn list_owned_in(
        transaction: &mut Transaction<'_, Postgres>,
        agent_id: &str,
        about_participant_id: Option<&str>,
    ) -> Result<Vec<ClimateView>, sqlx::Error> {
        sqlx::query_as::<_, ClimateRow>(
            "SELECT climate.agent_id, climate.about_participant_id,
                    climate.affinity, climate.trust, climate.last_note,
                    to_char(climate.updated_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US') || '+08:00'
                        AS updated_at
             FROM collab_agent_climates climate
             WHERE climate.agent_id = $1
               AND ($2::TEXT IS NULL OR climate.about_participant_id = $2)
             ORDER BY climate.about_participant_id",
        )
        .bind(agent_id)
        .bind(about_participant_id)
        .fetch_all(&mut **transaction)
        .await
        .map(|rows| rows.into_iter().map(ClimateView::from).collect())
    }

    pub(crate) async fn for_participants(
        pool: &PgPool,
        agent_id: &str,
        participant_ids: &[String],
    ) -> Result<Vec<ClimateView>, sqlx::Error> {
        if participant_ids.is_empty() {
            return Ok(Vec::new());
        }
        sqlx::query_as::<_, ClimateRow>(
            "SELECT climate.agent_id, climate.about_participant_id,
                    climate.affinity, climate.trust, climate.last_note,
                    to_char(climate.updated_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US') || '+08:00'
                        AS updated_at
             FROM collab_agent_climates climate
             WHERE climate.agent_id = $1
               AND climate.about_participant_id = ANY($2)
             ORDER BY climate.about_participant_id",
        )
        .bind(agent_id)
        .bind(participant_ids)
        .fetch_all(pool)
        .await
        .map(|rows| rows.into_iter().map(ClimateView::from).collect())
    }

    pub(crate) async fn overwrite_in(
        transaction: &mut Transaction<'_, Postgres>,
        agent_id: &str,
        about_participant_id: &str,
        affinity: f64,
        trust: f64,
        note: &str,
    ) -> Result<ClimateView, ClimateOperationError> {
        if agent_id == about_participant_id {
            return Err(domain(
                "INVALID_ARGUMENT",
                "cannot record Climate about yourself",
            ));
        }
        if !valid_score(affinity) || !valid_score(trust) {
            return Err(domain(
                "INVALID_ARGUMENT",
                "Climate affinity and trust must be finite values between -1 and 1",
            ));
        }
        let note = note.trim();
        if note.is_empty() || note.len() > CLIMATE_NOTE_MAX_BYTES || note.as_bytes().contains(&0) {
            return Err(domain(
                "INVALID_ARGUMENT",
                "Climate note must contain 1..4000 valid bytes",
            ));
        }
        let participant_exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM collab_participants WHERE id = $1)")
                .bind(about_participant_id)
                .fetch_one(&mut **transaction)
                .await?;
        if !participant_exists {
            return Err(domain("NOT_FOUND", "participant does not exist"));
        }
        let row = sqlx::query_as::<_, ClimateRow>(
            "INSERT INTO collab_agent_climates (
                agent_id, about_participant_id, affinity, trust, last_note
             ) VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (agent_id, about_participant_id) DO UPDATE
             SET affinity = EXCLUDED.affinity,
                 trust = EXCLUDED.trust,
                 last_note = EXCLUDED.last_note,
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             RETURNING agent_id, about_participant_id, affinity, trust, last_note,
                 to_char(updated_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US') || '+08:00'
                     AS updated_at",
        )
        .bind(agent_id)
        .bind(about_participant_id)
        .bind(affinity)
        .bind(trust)
        .bind(note)
        .fetch_one(&mut **transaction)
        .await?;
        Ok(ClimateView::from(row))
    }
}

fn valid_score(value: f64) -> bool {
    value.is_finite() && (-1.0..=1.0).contains(&value)
}

#[derive(FromRow)]
struct ClimateRow {
    agent_id: String,
    about_participant_id: String,
    affinity: f64,
    trust: f64,
    last_note: Option<String>,
    updated_at: String,
}

impl From<ClimateRow> for ClimateView {
    fn from(row: ClimateRow) -> Self {
        Self {
            agent_id: row.agent_id,
            about_participant_id: row.about_participant_id,
            affinity: row.affinity,
            trust: row.trust,
            last_note: row.last_note,
            updated_at: row.updated_at,
        }
    }
}

pub(crate) enum ClimateOperationError {
    Domain {
        code: &'static str,
        message: &'static str,
    },
    Database(sqlx::Error),
}

impl From<sqlx::Error> for ClimateOperationError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

fn domain(code: &'static str, message: &'static str) -> ClimateOperationError {
    ClimateOperationError::Domain { code, message }
}

#[cfg(test)]
mod tests {
    use super::valid_score;

    #[test]
    fn climate_scores_are_finite_and_bounded() {
        assert!(valid_score(-1.0));
        assert!(valid_score(0.0));
        assert!(valid_score(1.0));
        assert!(!valid_score(-1.000_001));
        assert!(!valid_score(1.000_001));
        assert!(!valid_score(f64::NAN));
        assert!(!valid_score(f64::INFINITY));
    }
}
