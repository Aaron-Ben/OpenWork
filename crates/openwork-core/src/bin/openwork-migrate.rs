use openwork_core::PostgresStorage;

#[tokio::main]
async fn main() {
    if let Err(error) = migrate().await {
        eprintln!("OpenWork database migration failed: {error}");
        std::process::exit(1);
    }
}

async fn migrate() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = std::env::var("DATABASE_URL").ok();
    let storage = PostgresStorage::connect(database_url.as_deref()).await?;
    storage.migrate().await?;
    Ok(())
}
