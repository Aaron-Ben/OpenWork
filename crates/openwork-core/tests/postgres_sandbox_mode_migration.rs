//! `202609240002_add_session_sandbox_mode` 对迁移前已有会话的取值（permissions.md §13.1、§13.3）。
//!
//! 空库上从头跑迁移测不到这一点：要先停在上一个迁移、写入旧数据，再单独执行这一个。
//! 为此在一个一次性的 schema 里逐个执行迁移文件，不碰共用测试库里的表。

use std::fs;
use std::path::PathBuf;

use sqlx::{Connection, PgConnection};
use uuid::Uuid;

const SANDBOX_MODE_MIGRATION: i64 = 202_609_240_002;

/// 版本号小于等于 `through` 的迁移文件，按版本排序。
fn migrations_through(through: i64) -> Vec<(i64, PathBuf)> {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let mut migrations = fs::read_dir(directory)
        .expect("migrations directory")
        .map(|entry| entry.expect("dir entry").path())
        .filter_map(|path| {
            let version = path
                .file_name()?
                .to_str()?
                .split('_')
                .next()?
                .parse::<i64>()
                .ok()?;
            (version <= through).then_some((version, path))
        })
        .collect::<Vec<_>>();
    migrations.sort();
    migrations
}

async fn execute_file(connection: &mut PgConnection, path: &PathBuf) {
    let sql = fs::read_to_string(path).expect("read migration");
    sqlx::raw_sql(&sql)
        .execute(&mut *connection)
        .await
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
}

/// 迁移前派生的 explorer 子会话取 `accept_edits`（explorer 的上限），根会话取 `auto`。
#[tokio::test]
async fn existing_sub_agents_get_their_role_ceiling_and_roots_get_auto() {
    let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
        return;
    };
    let mut connection = PgConnection::connect(&database_url).await.expect("connect");
    let schema = format!("sandbox_mode_migration_{}", Uuid::new_v4().simple());
    sqlx::raw_sql(&format!(
        "CREATE SCHEMA {schema}; SET search_path TO {schema};"
    ))
    .execute(&mut connection)
    .await
    .expect("scratch schema");

    let migrations = migrations_through(SANDBOX_MODE_MIGRATION);
    let (last_version, last_path) = migrations.last().expect("sandbox mode migration");
    assert_eq!(*last_version, SANDBOX_MODE_MIGRATION);
    for (_, path) in &migrations[..migrations.len() - 1] {
        execute_file(&mut connection, path).await;
    }
    sqlx::raw_sql(
        "INSERT INTO sessions (id, working_directory) VALUES ('root', '/w');
         INSERT INTO sessions (id, working_directory, parent_session_id, task_name, agent_role)
             VALUES ('child', '/w', 'root', 'inspect_code', 'explorer');",
    )
    .execute(&mut connection)
    .await
    .expect("sessions from before the migration");
    execute_file(&mut connection, last_path).await;

    let modes: Vec<(String, String)> =
        sqlx::query_as("SELECT id, sandbox_mode FROM sessions ORDER BY id")
            .fetch_all(&mut connection)
            .await
            .expect("modes");
    sqlx::raw_sql(&format!("DROP SCHEMA {schema} CASCADE;"))
        .execute(&mut connection)
        .await
        .expect("drop scratch schema");

    assert_eq!(
        modes,
        [
            ("child".to_string(), "accept_edits".to_string()),
            ("root".to_string(), "auto".to_string()),
        ]
    );
}
