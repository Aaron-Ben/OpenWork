# 旧版 Rust 代码

`crates/` 与 `desktop/` 是旧版 OpenWork（Rust 加 Tauri）。新版 Crew 开发期间只修影响旧版本运行的问题，不加功能，最后一步删除。设计见[重写的 Agent Note](../.agents/notes/proposed/architecture/2026-10-04-typescript-rewrite.md)。

## 检查

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --no-fail-fast
pnpm --dir desktop typecheck
pnpm --dir desktop test
```

运行 `cargo test` 前先设置 `TEST_DATABASE_URL` 与 `TEST_REDIS_URL`。没有设置时，依赖 PostgreSQL 的测试会静默跳过，结果不可信。

旧版的子系统页是 [collaboration.md](subsystems/collaboration.md) 与 [collaboration-desktop.md](subsystems/collaboration-desktop.md)，决策记录在 `.agents/notes/legacy/`，开发期间都保留作参考。
