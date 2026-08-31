use sqlx::{FromRow, PgPool};

use crate::protocol::{AgentAssignment, AgentView};

#[derive(Clone)]
pub(crate) struct Agents {
    pool: PgPool,
}

impl Agents {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) async fn create(
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

    pub(crate) async fn set_proactivity(
        &self,
        agent_id: &str,
        enabled: bool,
    ) -> Result<AgentView, sqlx::Error> {
        sqlx::query_as::<_, AgentViewRow>(
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
        .await
        .map(AgentView::from)
    }

    pub(crate) async fn list(&self) -> Result<Vec<AgentView>, sqlx::Error> {
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

    pub(crate) async fn assignments(&self) -> Result<Vec<AgentAssignment>, sqlx::Error> {
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
