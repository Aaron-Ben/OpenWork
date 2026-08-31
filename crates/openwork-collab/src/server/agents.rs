use sqlx::{FromRow, PgPool, Postgres, Transaction};
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};
use uuid::Uuid;

use crate::protocol::{AgentAssignment, AgentView};

#[derive(Clone)]
pub(crate) struct Agents {
    pool: PgPool,
}

impl Agents {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn create_in(
        transaction: &mut Transaction<'_, Postgres>,
        display_name: &str,
        role: Option<&str>,
        persona: &str,
        engine_id: &str,
        main_model_id: &str,
        triage_model_id: &str,
    ) -> Result<AgentView, sqlx::Error> {
        let display_name = required(display_name, "display name")?;
        let persona = required(persona, "persona")?;
        let engine_id = required(engine_id, "Engine id")?;
        let main_model_id = required(main_model_id, "main model id")?;
        let triage_model_id = required(triage_model_id, "triage model id")?;
        if engine_id != "opencode" {
            return Err(protocol_error("INVALID_ARGUMENT: unsupported Engine"));
        }
        let base = agent_slug(display_name);
        for attempt in 0..32 {
            let id = if attempt == 0 {
                base.clone()
            } else {
                format!("{base}-{}", &Uuid::new_v4().simple().to_string()[..4])
            };
            let inserted = sqlx::query(
                "INSERT INTO collab_participants (id, kind, display_name)
                 VALUES ($1, 'agent', $2)
                 ON CONFLICT (id) DO NOTHING",
            )
            .bind(&id)
            .bind(display_name)
            .execute(&mut **transaction)
            .await?;
            if inserted.rows_affected() == 0 {
                continue;
            }
            sqlx::query(
                "INSERT INTO collab_agent_profiles (agent_id, role, persona)
                 VALUES ($1, $2, $3)",
            )
            .bind(&id)
            .bind(role.map(str::trim).filter(|value| !value.is_empty()))
            .bind(persona)
            .execute(&mut **transaction)
            .await?;
            sqlx::query(
                "INSERT INTO collab_agent_runtime_configs (
                    agent_id, engine_id, main_model_id, triage_model_id
                 ) VALUES ($1, $2, $3, $4)",
            )
            .bind(&id)
            .bind(engine_id)
            .bind(main_model_id)
            .bind(triage_model_id)
            .execute(&mut **transaction)
            .await?;
            return Self::get_in(transaction, &id).await;
        }
        Err(protocol_error(
            "CONFLICT: could not allocate a unique Agent id",
        ))
    }

    pub(crate) async fn set_agenda_in(
        transaction: &mut Transaction<'_, Postgres>,
        agent_id: &str,
        enabled: bool,
    ) -> Result<AgentView, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE collab_agent_runtime_configs config
             SET agenda_enabled = $2,
                 config_revision = config_revision + 1,
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             FROM collab_agent_profiles profile
             WHERE config.agent_id = $1 AND profile.agent_id = config.agent_id
               AND profile.archived_at IS NULL",
        )
        .bind(agent_id)
        .bind(enabled)
        .execute(&mut **transaction)
        .await?;
        if result.rows_affected() == 0 {
            return Err(sqlx::Error::RowNotFound);
        }
        Self::get_in(transaction, agent_id).await
    }

    pub(crate) async fn set_archived_in(
        transaction: &mut Transaction<'_, Postgres>,
        agent_id: &str,
        archived: bool,
    ) -> Result<AgentView, sqlx::Error> {
        let result = sqlx::query(
            "UPDATE collab_agent_profiles
             SET archived_at = CASE WHEN $2
                     THEN COALESCE(archived_at, CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai')
                     ELSE NULL
                 END,
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             WHERE agent_id = $1",
        )
        .bind(agent_id)
        .bind(archived)
        .execute(&mut **transaction)
        .await?;
        if result.rows_affected() == 0 {
            return Err(sqlx::Error::RowNotFound);
        }
        sqlx::query(
            "UPDATE collab_agent_runtime_configs
             SET config_revision = config_revision + 1,
                 updated_at = CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'
             WHERE agent_id = $1",
        )
        .bind(agent_id)
        .execute(&mut **transaction)
        .await?;
        Self::get_in(transaction, agent_id).await
    }

    pub(crate) async fn get_in(
        transaction: &mut Transaction<'_, Postgres>,
        agent_id: &str,
    ) -> Result<AgentView, sqlx::Error> {
        sqlx::query_as::<_, AgentViewRow>(&agent_view_query("WHERE participant.id = $1"))
            .bind(agent_id)
            .fetch_one(&mut **transaction)
            .await
            .map(AgentView::from)
    }

    pub(crate) async fn list(&self) -> Result<Vec<AgentView>, sqlx::Error> {
        sqlx::query_as::<_, AgentViewRow>(&agent_view_query("ORDER BY participant.id"))
            .fetch_all(&self.pool)
            .await
            .map(|rows| rows.into_iter().map(AgentView::from).collect())
    }

    pub(crate) async fn assignments(&self) -> Result<Vec<AgentAssignment>, sqlx::Error> {
        sqlx::query_as::<_, AssignmentRow>(
            "SELECT participant.id, participant.display_name, profile.role, profile.persona,
                    config.engine_id, config.main_model_id, config.triage_model_id,
                    config.config_revision, config.agenda_enabled
             FROM collab_agent_profiles profile
             JOIN collab_participants participant ON participant.id = profile.agent_id
             JOIN collab_agent_runtime_configs config ON config.agent_id = profile.agent_id
             WHERE profile.archived_at IS NULL
             ORDER BY participant.id",
        )
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(AgentAssignment::from).collect())
    }

    pub(crate) async fn is_active(&self, agent_id: &str) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM collab_agent_profiles
                WHERE agent_id = $1 AND archived_at IS NULL
             )",
        )
        .bind(agent_id)
        .fetch_one(&self.pool)
        .await
    }
}

pub(crate) fn agent_slug(display_name: &str) -> String {
    let mut slug = String::new();
    let mut separator_pending = false;
    for character in display_name.nfkd() {
        if is_combining_mark(character) {
            continue;
        }
        if character.is_ascii_alphanumeric() {
            if separator_pending && !slug.is_empty() && slug.len() < 24 {
                slug.push('-');
            }
            separator_pending = false;
            if slug.len() < 24 {
                slug.push(character.to_ascii_lowercase());
            }
        } else if !slug.is_empty() {
            separator_pending = true;
        }
        if slug.len() >= 24 {
            break;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        slug = "agent".to_string();
    } else if !slug.as_bytes()[0].is_ascii_alphabetic() {
        slug = format!("a-{slug}");
    }
    slug
}

fn agent_view_query(suffix: &str) -> String {
    format!(
        "SELECT participant.id, participant.display_name, profile.role, profile.persona,
                config.engine_id, config.main_model_id, config.triage_model_id,
                config.config_revision, config.agenda_enabled,
                CASE WHEN profile.archived_at IS NULL THEN NULL ELSE
                    to_char(profile.archived_at, 'YYYY-MM-DD\"T\"HH24:MI:SS') || '+08:00'
                END AS archived_at
         FROM collab_agent_profiles profile
         JOIN collab_participants participant ON participant.id = profile.agent_id
         JOIN collab_agent_runtime_configs config ON config.agent_id = profile.agent_id
         {suffix}"
    )
}

fn required<'a>(value: &'a str, name: &str) -> Result<&'a str, sqlx::Error> {
    let value = value.trim();
    if value.is_empty() {
        Err(protocol_error(&format!(
            "INVALID_ARGUMENT: {name} cannot be empty"
        )))
    } else {
        Ok(value)
    }
}

#[derive(FromRow)]
struct AgentViewRow {
    id: String,
    display_name: String,
    role: Option<String>,
    persona: String,
    engine_id: String,
    main_model_id: String,
    triage_model_id: String,
    config_revision: i64,
    agenda_enabled: bool,
    archived_at: Option<String>,
}

impl From<AgentViewRow> for AgentView {
    fn from(row: AgentViewRow) -> Self {
        Self {
            id: row.id,
            display_name: row.display_name,
            role: row.role,
            persona: row.persona,
            engine_id: row.engine_id,
            main_model_id: row.main_model_id,
            triage_model_id: row.triage_model_id,
            config_revision: row.config_revision,
            agenda_enabled: row.agenda_enabled,
            archived_at: row.archived_at,
        }
    }
}

#[derive(FromRow)]
struct AssignmentRow {
    id: String,
    display_name: String,
    role: Option<String>,
    persona: String,
    engine_id: String,
    main_model_id: String,
    triage_model_id: String,
    config_revision: i64,
    agenda_enabled: bool,
}

impl From<AssignmentRow> for AgentAssignment {
    fn from(row: AssignmentRow) -> Self {
        Self {
            id: row.id,
            display_name: row.display_name,
            role: row.role,
            persona: row.persona,
            engine_id: row.engine_id,
            main_model_id: row.main_model_id,
            triage_model_id: row.triage_model_id,
            config_revision: row.config_revision,
            agenda_enabled: row.agenda_enabled,
        }
    }
}

fn protocol_error(message: &str) -> sqlx::Error {
    sqlx::Error::Protocol(message.to_string())
}

#[cfg(test)]
mod tests {
    use super::agent_slug;

    #[test]
    fn slug_is_stable_ascii_bounded_and_starts_with_a_letter() {
        assert_eq!(agent_slug("Équipe Démo"), "equipe-demo");
        assert_eq!(agent_slug("123 Helper"), "a-123-helper");
        assert_eq!(agent_slug("小明"), "agent");
        assert_eq!(
            agent_slug("A very very very very long name"),
            "a-very-very-very-very-lo"
        );
    }
}
