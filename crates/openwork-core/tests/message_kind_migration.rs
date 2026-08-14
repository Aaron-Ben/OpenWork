//! `MessageKind` 与数据库 CHECK 约束必须始终一致。
//!
//! 这两处分别在 Rust 和 SQL 里各写一遍同一个取值集合，天然会漂移：加了枚举
//! 变体忘了写迁移，要等到运行时插入被 Postgres 拒绝才暴露，而且只在真的写了
//! 那种消息的代码路径上暴露。
//!
//! 本测试**不需要数据库**：它直接读迁移文件。`crates/openwork-core/tests/` 下
//! 其余 postgres 测试在 `TEST_DATABASE_URL` 未设置时会静默返回，因此不能指望
//! 它们守住这条。

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use openwork_core::MessageKind;

fn migrations_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("migrations")
}

/// 枚举里的全部取值。
///
/// 下面的穷尽 `match` 是这个列表的守卫：新增变体时它会编译失败，逼作者回到
/// 这里，而不是让测试悄悄少测一个。
fn every_message_kind() -> BTreeSet<&'static str> {
    let all = [
        MessageKind::Normal,
        MessageKind::SkillInstruction,
        MessageKind::AgentMessage,
        MessageKind::WorldState,
    ];
    for kind in all {
        match kind {
            MessageKind::Normal
            | MessageKind::SkillInstruction
            | MessageKind::AgentMessage
            | MessageKind::WorldState => {}
        }
    }
    all.into_iter().map(MessageKind::as_str).collect()
}

/// 从全部迁移里取**最后一次**定义 `messages_kind_valid` 的取值集合。
///
/// 取最后一次而不是第一次：约束是靠 DROP + ADD 重建的，空库跑完全部迁移后
/// 生效的是最后那一条。
fn effective_message_kind_check() -> BTreeSet<String> {
    let mut files: Vec<_> = fs::read_dir(migrations_dir())
        .expect("migrations directory")
        .map(|entry| entry.expect("dir entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "sql"))
        .collect();
    files.sort();

    let mut effective = None;
    for path in files {
        let sql = fs::read_to_string(&path).expect("read migration");
        let mut rest = sql.as_str();
        while let Some(at) = rest.find("messages_kind_valid") {
            rest = &rest[at + "messages_kind_valid".len()..];
            let Some(open) = rest.find("IN (") else {
                continue;
            };
            let after = &rest[open + "IN (".len()..];
            let Some(close) = after.find(')') else {
                panic!("unterminated IN list in {}", path.display());
            };
            effective = Some(
                after[..close]
                    .split(',')
                    .map(|value| value.trim().trim_matches('\'').to_string())
                    .filter(|value| !value.is_empty())
                    .collect::<BTreeSet<_>>(),
            );
            rest = &after[close..];
        }
    }
    effective.expect("no migration defines messages_kind_valid")
}

/// 枚举取值与 CHECK 取值必须完全相等。
///
/// 少了：写这种消息时被 Postgres 拒绝，Turn 失败。
/// 多了：库里可能存在 Rust 解析不了的行，读取时整个会话加载失败。
#[test]
fn every_message_kind_is_accepted_by_the_database() {
    let expected = every_message_kind();
    let allowed = effective_message_kind_check();

    let missing: Vec<_> = expected
        .iter()
        .filter(|kind| !allowed.contains(**kind))
        .collect();
    assert!(
        missing.is_empty(),
        "这些 MessageKind 会被数据库拒绝，需要新增迁移放宽 messages_kind_valid：{missing:?}"
    );

    let unexpected: Vec<_> = allowed
        .iter()
        .filter(|value| !expected.contains(value.as_str()))
        .collect();
    assert!(
        unexpected.is_empty(),
        "CHECK 允许了 Rust 解析不了的取值：{unexpected:?}"
    );
}

/// 已应用的迁移不可修改，只能新增（见 .claude/rules/database.md）。
///
/// 放宽取值必须落在一个新文件里；直接改 202608080001 那条会让已经跑过迁移的
/// 库与迁移文件的校验和对不上。
#[test]
fn the_kind_constraint_is_widened_by_a_new_migration() {
    let mut definitions: Vec<_> = fs::read_dir(migrations_dir())
        .expect("migrations directory")
        .map(|entry| entry.expect("dir entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "sql"))
        .filter(|path| {
            fs::read_to_string(path)
                .expect("read migration")
                .contains("messages_kind_valid")
        })
        .collect();
    definitions.sort();

    let latest = definitions
        .last()
        .expect("a migration defines the constraint");
    let sql = fs::read_to_string(latest).expect("read migration");
    assert!(
        sql.contains("world_state"),
        "放宽 world_state 必须写在新的迁移文件里，当前最后一处定义在 {}",
        latest.display()
    );
    assert!(
        !latest.ends_with("202608080001_add_subagent_sessions.sql"),
        "不能修改已应用的迁移 202608080001，必须新增文件"
    );
}
