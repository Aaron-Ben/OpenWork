use std::time::Duration;

use sqlx::{
    Arguments, Executor, FromRow, PgPool, Postgres,
    postgres::{PgArguments, PgPoolOptions, PgRow},
    query::QueryAs,
};
use thiserror::Error;
use time::{OffsetDateTime, UtcOffset};

const DEFAULT_DATABASE_URL: &str = "postgres://openwork:openwork@localhost:5432/openwork";

#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
    pub acquire_timeout: Duration,
}

impl DatabaseConfig {
    pub fn from_env_or_local() -> Self {
        Self {
            url: std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string()),
            max_connections: 8,
            acquire_timeout: Duration::from_secs(10),
        }
    }
}

#[derive(Clone)]
pub struct Database {
    pool: PgPool,
}

impl Database {
    pub async fn connect(config: DatabaseConfig) -> Result<Self, DatabaseError> {
        let pool = PgPoolOptions::new()
            .max_connections(config.max_connections)
            .acquire_timeout(config.acquire_timeout)
            .after_connect(|conn, _meta| {
                Box::pin(async move {
                    conn.execute("SET TIME ZONE 'Asia/Shanghai'").await?;
                    Ok(())
                })
            })
            .connect(&config.url)
            .await?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn migrate(&self, migrations: &[Migration]) -> Result<(), DatabaseError> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
               version BIGINT PRIMARY KEY,
               name TEXT NOT NULL,
               applied_at TIMESTAMPTZ NOT NULL
             )",
        )
        .execute(&self.pool)
        .await?;

        for migration in migrations {
            let exists: Option<i64> =
                sqlx::query_scalar("SELECT 1::BIGINT FROM schema_migrations WHERE version = $1")
                    .bind(migration.version)
                    .fetch_optional(&self.pool)
                    .await?;
            if exists.is_some() {
                continue;
            }

            let mut tx = self.pool.begin().await?;
            for statement in migration.statements {
                sqlx::query(statement).execute(&mut *tx).await?;
            }
            sqlx::query(
                "INSERT INTO schema_migrations (version, name, applied_at)
                 VALUES ($1, $2, $3)",
            )
            .bind(migration.version)
            .bind(migration.name)
            .bind(now_beijing())
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
        }

        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("postgres error: {0}")]
    Sqlx(#[from] sqlx::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Migration {
    pub version: i64,
    pub name: &'static str,
    pub statements: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PgColumn {
    pub name: &'static str,
    pub rust_type: &'static str,
    pub primary_key: bool,
    pub indexed: bool,
    pub nullable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PgIndex {
    pub name: &'static str,
    pub columns: &'static [&'static str],
}

pub trait PgSchema: Send + Sync + Unpin + Clone + std::fmt::Debug + Sized {
    type Id: for<'q> sqlx::Encode<'q, Postgres> + sqlx::Type<Postgres> + Clone + Send + Sync;
    type Row: for<'r> FromRow<'r, PgRow> + Send + Unpin;

    const TABLE: &'static str;
    const ID_COLUMN: &'static str;
    const COLUMNS: &'static [PgColumn];
    const INDEXES: &'static [PgIndex];

    fn get_id_value(&self) -> Self::Id;

    fn from_row(row: Self::Row) -> Self;

    fn primary_key() -> Option<&'static PgColumn> {
        Self::COLUMNS.iter().find(|column| column.primary_key)
    }

    fn column_names() -> Vec<&'static str> {
        Self::COLUMNS.iter().map(|column| column.name).collect()
    }

    fn select_list() -> String {
        Self::column_names().join(", ")
    }

    fn has_column(column: &str) -> bool {
        Self::COLUMNS
            .iter()
            .any(|candidate| candidate.name == column)
    }
}

#[async_trait::async_trait]
pub trait PgCrud: PgSchema {
    fn bind_insert<'q>(
        &'q self,
        query: QueryAs<'q, Postgres, Self::Row, PgArguments>,
    ) -> QueryAs<'q, Postgres, Self::Row, PgArguments>;

    fn bind_update<'q>(
        &'q self,
        query: QueryAs<'q, Postgres, Self::Row, PgArguments>,
    ) -> QueryAs<'q, Postgres, Self::Row, PgArguments>;

    fn select_by_primary_key_sql() -> String {
        format!(
            "SELECT {} FROM {} WHERE {} = $1",
            Self::select_list(),
            Self::TABLE,
            Self::ID_COLUMN
        )
    }

    fn insert_sql() -> String {
        let columns = Self::column_names();
        let placeholders = (1..=columns.len())
            .map(|index| format!("${index}"))
            .collect::<Vec<_>>();
        format!(
            "INSERT INTO {} ({}) VALUES ({}) RETURNING {}",
            Self::TABLE,
            columns.join(", "),
            placeholders.join(", "),
            Self::select_list()
        )
    }

    fn update_by_primary_key_sql() -> String {
        let assignments = Self::COLUMNS
            .iter()
            .filter(|column| !column.primary_key)
            .enumerate()
            .map(|(index, column)| format!("{} = ${}", column.name, index + 1))
            .collect::<Vec<_>>();
        format!(
            "UPDATE {} SET {} WHERE {} = ${} RETURNING {}",
            Self::TABLE,
            assignments.join(", "),
            Self::ID_COLUMN,
            assignments.len() + 1,
            Self::select_list()
        )
    }

    fn delete_by_primary_key_sql() -> String {
        format!("DELETE FROM {} WHERE {} = $1", Self::TABLE, Self::ID_COLUMN)
    }

    async fn create<'e, E>(self, executor: E) -> Result<Self, sqlx::Error>
    where
        E: Executor<'e, Database = Postgres> + Send,
        Self: Send,
    {
        let sql = Self::insert_sql();
        self.bind_insert(sqlx::query_as::<_, Self::Row>(&sql))
            .fetch_one(executor)
            .await
            .map(Self::from_row)
    }

    async fn update<'e, E>(self, executor: E) -> Result<Self, sqlx::Error>
    where
        E: Executor<'e, Database = Postgres> + Send,
        Self: Send,
    {
        let sql = Self::update_by_primary_key_sql();
        self.bind_update(sqlx::query_as::<_, Self::Row>(&sql))
            .fetch_one(executor)
            .await
            .map(Self::from_row)
    }

    async fn delete<'e, E>(self, executor: E) -> Result<u64, sqlx::Error>
    where
        E: Executor<'e, Database = Postgres> + Send,
        Self: Send,
    {
        sqlx::query(&Self::delete_by_primary_key_sql())
            .bind(self.get_id_value())
            .execute(executor)
            .await
            .map(|done| done.rows_affected())
    }

    async fn get_by_id<'e, E>(id: Self::Id, executor: E) -> Result<Option<Self>, sqlx::Error>
    where
        E: Executor<'e, Database = Postgres> + Send,
        Self: Send,
    {
        sqlx::query_as::<_, Self::Row>(&Self::select_by_primary_key_sql())
            .bind(id)
            .fetch_optional(executor)
            .await
            .map(|row| row.map(Self::from_row))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderDirection {
    Asc,
    Desc,
}

impl OrderDirection {
    pub fn as_sql(self) -> &'static str {
        match self {
            Self::Asc => "ASC",
            Self::Desc => "DESC",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterOperator {
    Eq,
    Ne,
    Gt,
    Gte,
    Lt,
    Lte,
    Like,
    IsNull,
    IsNotNull,
}

impl FilterOperator {
    pub fn as_sql(self) -> &'static str {
        match self {
            Self::Eq => "=",
            Self::Ne => "<>",
            Self::Gt => ">",
            Self::Gte => ">=",
            Self::Lt => "<",
            Self::Lte => "<=",
            Self::Like => "LIKE",
            Self::IsNull => "IS NULL",
            Self::IsNotNull => "IS NOT NULL",
        }
    }

    pub fn needs_value(self) -> bool {
        !matches!(self, Self::IsNull | Self::IsNotNull)
    }
}

pub trait PgArgument: Send + Sync {
    fn add_to_args(&self, args: &mut PgArguments) -> Result<(), sqlx::Error>;
}

impl<T> PgArgument for T
where
    T: for<'q> sqlx::Encode<'q, Postgres> + sqlx::Type<Postgres> + Clone + Send + Sync + 'static,
{
    fn add_to_args(&self, args: &mut PgArguments) -> Result<(), sqlx::Error> {
        args.add(self.clone()).map_err(sqlx::Error::Encode)
    }
}

pub struct FilterCondition {
    pub column: &'static str,
    pub operator: FilterOperator,
    pub value: Option<Box<dyn PgArgument>>,
}

#[derive(Default)]
pub struct QueryCriteria {
    pub conditions: Vec<FilterCondition>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub order_by: Vec<(&'static str, OrderDirection)>,
}

impl QueryCriteria {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn filter<V>(
        mut self,
        column: &'static str,
        operator: FilterOperator,
        value: Option<V>,
    ) -> Self
    where
        V: for<'q> sqlx::Encode<'q, Postgres>
            + sqlx::Type<Postgres>
            + Clone
            + Send
            + Sync
            + 'static,
    {
        self.conditions.push(FilterCondition {
            column,
            operator,
            value: value.map(|value| Box::new(value) as Box<dyn PgArgument>),
        });
        self
    }

    pub fn eq<V>(self, column: &'static str, value: V) -> Self
    where
        V: for<'q> sqlx::Encode<'q, Postgres>
            + sqlx::Type<Postgres>
            + Clone
            + Send
            + Sync
            + 'static,
    {
        self.filter(column, FilterOperator::Eq, Some(value))
    }

    pub fn is_null(mut self, column: &'static str) -> Self {
        self.conditions.push(FilterCondition {
            column,
            operator: FilterOperator::IsNull,
            value: None,
        });
        self
    }

    pub fn is_not_null(mut self, column: &'static str) -> Self {
        self.conditions.push(FilterCondition {
            column,
            operator: FilterOperator::IsNotNull,
            value: None,
        });
        self
    }

    pub fn order_by(mut self, column: &'static str, direction: OrderDirection) -> Self {
        self.order_by.push((column, direction));
        self
    }

    pub fn limit(mut self, limit: i64) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn offset(mut self, offset: i64) -> Self {
        self.offset = Some(offset);
        self
    }
}

#[async_trait::async_trait]
pub trait PgFilterQuery: PgSchema {
    async fn find_by_criteria<'e, E>(
        criteria: QueryCriteria,
        executor: E,
    ) -> Result<Vec<Self>, sqlx::Error>
    where
        E: Executor<'e, Database = Postgres> + Send,
        Self: Send,
    {
        let (sql, arguments) = build_select_query::<Self>(criteria)?;
        sqlx::query_as_with::<_, Self::Row, _>(&sql, arguments)
            .fetch_all(executor)
            .await
            .map(|rows| rows.into_iter().map(Self::from_row).collect())
    }

    async fn find_one_by_criteria<'e, E>(
        criteria: QueryCriteria,
        executor: E,
    ) -> Result<Option<Self>, sqlx::Error>
    where
        E: Executor<'e, Database = Postgres> + Send,
        Self: Send,
    {
        let mut records = Self::find_by_criteria(criteria.limit(1), executor).await?;
        Ok(records.pop())
    }

    async fn delete_by_criteria<'e, E>(
        criteria: QueryCriteria,
        executor: E,
    ) -> Result<u64, sqlx::Error>
    where
        E: Executor<'e, Database = Postgres> + Send,
        Self: Send,
    {
        let (sql, arguments) = build_delete_query::<Self>(criteria)?;
        sqlx::query_with(&sql, arguments)
            .execute(executor)
            .await
            .map(|done| done.rows_affected())
    }
}

impl<T> PgFilterQuery for T where T: PgSchema {}

fn build_select_query<T: PgSchema>(
    criteria: QueryCriteria,
) -> Result<(String, PgArguments), sqlx::Error> {
    let mut parts = vec![format!("SELECT {} FROM {}", T::select_list(), T::TABLE)];
    let mut arguments = PgArguments::default();
    let mut next_placeholder = 1;
    append_where::<T>(
        &mut parts,
        &mut arguments,
        &mut next_placeholder,
        &criteria.conditions,
    )?;

    if !criteria.order_by.is_empty() {
        let mut order_clauses = Vec::with_capacity(criteria.order_by.len());
        for (column, direction) in criteria.order_by {
            validate_column::<T>(column)?;
            order_clauses.push(format!("{} {}", column, direction.as_sql()));
        }
        parts.push(format!("ORDER BY {}", order_clauses.join(", ")));
    }

    if let Some(limit) = criteria.limit {
        arguments.add(limit).map_err(sqlx::Error::Encode)?;
        parts.push(format!("LIMIT ${next_placeholder}"));
        next_placeholder += 1;
    }

    if let Some(offset) = criteria.offset {
        arguments.add(offset).map_err(sqlx::Error::Encode)?;
        parts.push(format!("OFFSET ${next_placeholder}"));
    }

    Ok((parts.join(" "), arguments))
}

fn build_delete_query<T: PgSchema>(
    criteria: QueryCriteria,
) -> Result<(String, PgArguments), sqlx::Error> {
    let mut parts = vec![format!("DELETE FROM {}", T::TABLE)];
    let mut arguments = PgArguments::default();
    let mut next_placeholder = 1;
    append_where::<T>(
        &mut parts,
        &mut arguments,
        &mut next_placeholder,
        &criteria.conditions,
    )?;
    Ok((parts.join(" "), arguments))
}

fn append_where<T: PgSchema>(
    parts: &mut Vec<String>,
    arguments: &mut PgArguments,
    next_placeholder: &mut i32,
    conditions: &[FilterCondition],
) -> Result<(), sqlx::Error> {
    if conditions.is_empty() {
        return Ok(());
    }

    let mut clauses = Vec::with_capacity(conditions.len());
    for condition in conditions {
        validate_column::<T>(condition.column)?;
        let mut clause = format!("{} {}", condition.column, condition.operator.as_sql());
        if condition.operator.needs_value() {
            let Some(value) = &condition.value else {
                return Err(sqlx::Error::Protocol(format!(
                    "filter for column {} requires a value",
                    condition.column
                )));
            };
            value.add_to_args(arguments)?;
            clause.push_str(&format!(" ${}", *next_placeholder));
            *next_placeholder += 1;
        }
        clauses.push(clause);
    }

    parts.push(format!("WHERE {}", clauses.join(" AND ")));
    Ok(())
}

fn validate_column<T: PgSchema>(column: &str) -> Result<(), sqlx::Error> {
    if T::has_column(column) {
        Ok(())
    } else {
        Err(sqlx::Error::ColumnNotFound(column.to_string()))
    }
}

pub type DbDateTime = OffsetDateTime;

pub fn now_beijing() -> DbDateTime {
    OffsetDateTime::now_utc().to_offset(beijing_offset())
}

pub fn epoch_seconds(timestamp: DbDateTime) -> i64 {
    timestamp.unix_timestamp()
}

fn beijing_offset() -> UtcOffset {
    UtcOffset::from_hms(8, 0, 0).expect("valid Beijing UTC offset")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone)]
    struct SessionRecord {
        id: String,
    }

    #[derive(sqlx::FromRow)]
    #[allow(dead_code)]
    struct SessionRecordRow {
        id: String,
        title: String,
        working_dir: Option<String>,
    }

    impl PgSchema for SessionRecord {
        type Id = String;
        type Row = SessionRecordRow;

        const TABLE: &'static str = "sessions";
        const ID_COLUMN: &'static str = "id";
        const COLUMNS: &'static [PgColumn] = &[
            PgColumn {
                name: "id",
                rust_type: "String",
                primary_key: true,
                indexed: false,
                nullable: false,
            },
            PgColumn {
                name: "title",
                rust_type: "String",
                primary_key: false,
                indexed: false,
                nullable: false,
            },
            PgColumn {
                name: "working_dir",
                rust_type: "Option < String >",
                primary_key: false,
                indexed: false,
                nullable: true,
            },
        ];
        const INDEXES: &'static [PgIndex] = &[];

        fn get_id_value(&self) -> Self::Id {
            self.id.clone()
        }

        fn from_row(row: Self::Row) -> Self {
            Self { id: row.id }
        }
    }

    impl PgCrud for SessionRecord {
        fn bind_insert<'q>(
            &'q self,
            query: QueryAs<'q, Postgres, Self::Row, PgArguments>,
        ) -> QueryAs<'q, Postgres, Self::Row, PgArguments> {
            query.bind(&self.id).bind("title").bind(None::<String>)
        }

        fn bind_update<'q>(
            &'q self,
            query: QueryAs<'q, Postgres, Self::Row, PgArguments>,
        ) -> QueryAs<'q, Postgres, Self::Row, PgArguments> {
            query.bind("title").bind(None::<String>).bind(&self.id)
        }
    }

    #[test]
    fn pg_crud_builds_basic_sql() {
        assert_eq!(
            SessionRecord::select_by_primary_key_sql(),
            "SELECT id, title, working_dir FROM sessions WHERE id = $1"
        );
        assert_eq!(
            SessionRecord::insert_sql(),
            "INSERT INTO sessions (id, title, working_dir) VALUES ($1, $2, $3) RETURNING id, title, working_dir"
        );
        assert_eq!(
            SessionRecord::update_by_primary_key_sql(),
            "UPDATE sessions SET title = $1, working_dir = $2 WHERE id = $3 RETURNING id, title, working_dir"
        );
        assert_eq!(
            SessionRecord::delete_by_primary_key_sql(),
            "DELETE FROM sessions WHERE id = $1"
        );
    }
}
