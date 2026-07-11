use openwork_persistence::PostgresPersistence;

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    if let Err(error) = PostgresPersistence::migrate_from_env_or_local().await {
        eprintln!("OpenWork database migration failed: {error}");
        std::process::exit(1);
    }
    println!("OpenWork database migration completed.");
}
