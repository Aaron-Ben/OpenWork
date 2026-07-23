# AGENTS.md

OpenWork: local agent workbench. Rust workspace (5 crates in `crates/`) + Tauri 2 / React / TypeScript desktop app in `apps/desktop/`. PostgreSQL-only persistence via SQLx.

## Commands

Rust — run from repo root:

```bash
cargo test                                 # workspace tests
cargo clippy --all-targets --all-features
cargo fmt
```

Desktop — run from `apps/desktop/` only. The repo root has no `package.json`; running pnpm scripts at root fails with `ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND`.

```bash
pnpm install
pnpm tauri dev      # desktop app (needs postgres + .env, see below)
pnpm test           # vitest, fast (~1s)
pnpm build          # tsc && vite build
```

Database (PostgreSQL 16 via docker compose):

```bash
docker compose up -d postgres
cargo run -p openwork-core --bin openwork-migrate   # from repo root
```

Start order: postgres → migrate → `pnpm tauri dev`. `OpenWorkCore::bootstrap` also applies pending migrations at startup; the migrate binary is for provisioning/diagnosis.

## Environment

- `.env` lives at the **repo root** (gitignored). Template: `.env.example`.
- `OPENWORK_API_KEY_ENCRYPTION_KEY`: base64-encoded 32 bytes, generate once with `openssl rand -base64 32`. **Never rotate it against an existing database** — stored provider API keys become undecryptable.
- Debug builds (`pnpm tauri dev`) auto-load the root `.env` via dotenvy; release builds do not.
- `DATABASE_URL` defaults to `postgres://openwork:openwork@localhost:5432/openwork` when unset.
- `Cargo.lock` is gitignored in this repo — do not commit it.

## Testing quirks

- `crates/openwork-core/tests/postgres_*.rs` require `TEST_DATABASE_URL`; without it they return early and **pass vacuously**. To run them for real:
  ```bash
  TEST_DATABASE_URL=postgres://openwork:openwork@localhost:5432/openwork cargo test -p openwork-core
  ```
- `deepseek_live_runtime.rs` is `#[ignore]`d (needs network + `DEEPSEEK_API_KEY` + `TEST_DATABASE_URL`).
- Tauri-side contract test: `apps/desktop/src-tauri/tests/command_error_contract.rs` pins the stable error code/message shape exposed to the frontend.

## Migrations

- Location: `crates/openwork-core/migrations/`. This is the schema source of truth.
- Never edit an applied migration; add a new file: `sqlx migrate add --source crates/openwork-core/migrations <description>`.
- Local reset (destroys all data): `docker compose down -v && docker compose up -d postgres && cargo run -p openwork-core --bin openwork-migrate`.

## Architecture invariants

Authoritative design docs: `docs/redesign/` (see `docs/README.md` index). Source of truth for "what exists now" is the code + migrations.

- `openwork-core` is the **only** runtime entry. `OpenWorkCore` owns provider repository, credentials, and the session registry. Tauri (`apps/desktop/src-tauri`) only adapts Commands/Events and maps errors — no state machine duplication.
- Dependency direction (never reversed): `openwork-models` ← `openwork-tools`, `openwork-chat-state` ← `openwork-agent` ← `openwork-core` ← `apps/desktop/src-tauri`. `openwork-models` depends on no other OpenWork crate.
- One active session = one `SessionActor`; it drives at most one Turn at a time. The actor's run loop is the only place that advances the Model → Tool/Permission → Model chain. Trace, storage, and desktop never advance a Turn.
- `openwork-chat-state` is the single writer of the conversation; core writes via commands, reads via snapshots.
- `openwork-agent` holds only static agent definition (prompt, tool set, limits) — it must not start async run loops.
- Trace is best-effort diagnostics; trace/queue/DB failures must not fail a Turn.
- Permission `Allow` cannot bypass `ToolSessionContext` path/process boundaries. There is no OS-level sandbox.
- Model is always explicitly chosen by the user (`providerId + model`); no auto-selection or cross-model fallback.

## Frontend ↔ Rust contract

- Contract codegen is deferred. Bridge DTOs are **hand-written** in `apps/desktop/src/bridge/compat.ts` and must be manually kept in sync with the Rust host DTOs (defined in `openwork-core`, e.g. `SessionInput` in `storage/postgres.rs`, surfaced via `src-tauri/src/commands/`). Bump `RUNTIME_SESSION_UPDATE_VERSION` in `compat.ts` when the update shape changes.
- React only talks to core through `apps/desktop/src/bridge/commands.ts` invoke wrappers — never SQL, provider adapters, or tool executors.
- Frontend state layers are separate: canonical session/messages, per-session runtime view (reducer), local UI state. See `docs/redesign/06-frontend-architecture.md`.

## Conventions

- Domain vocabulary: Session, Turn, Model Call, Tool Call, Permission, Message, Update, Trace, Compaction. Do not reintroduce retired terms (`StepId`, `ToolRunId`, `ApprovalId`, `JournalTurnRecorder`, Event Journal).
- Manual Conversation compaction is in V1 scope: it must be explicitly invoked with `/compact`, only run while the Session is idle, and replace only the Conversation projection after success. Do not add automatic thresholds, overflow-triggered compaction/resubmission, lossy fallback, or cross-process compaction replay.
- Explicit V1 non-goals — do not add: MCP, Memory, Plan, Skill, Artifact, Git/Diff, Worktree, cross-process Turn recovery, Event Journal.
- Root README is bilingual: when editing `README.md`, mirror changes in `README.en.md`.
