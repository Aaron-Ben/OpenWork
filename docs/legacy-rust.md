# 旧版 Rust 代码

`crates/` 与 `desktop/` 是旧版 OpenWork（Rust 加 Tauri）。新版 Crew 开发期间只修影响旧版本运行的问题，不加功能，最后一步删除。设计见[重写的 Agent Note](../.agents/notes/proposed/architecture/2026-10-04-typescript-rewrite.md)。

## 检查

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
pnpm --dir desktop typecheck
scripts/check.sh    # 全量测试，先设置 TEST_DATABASE_URL 与 TEST_REDIS_URL
```

旧版的子系统页是 [collaboration.md](subsystems/collaboration.md) 与 [collaboration-desktop.md](subsystems/collaboration-desktop.md)，开发期间保留作参考。
