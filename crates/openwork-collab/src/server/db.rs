use sqlx::PgPool;

use super::migration;

pub(crate) async fn connect(database_url: &str) -> Result<PgPool, sqlx::Error> {
    let pool = PgPool::connect(database_url).await?;
    migration::migrate(&pool).await?;
    Ok(pool)
}
