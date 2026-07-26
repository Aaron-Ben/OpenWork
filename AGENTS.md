# AGENTS.md

OpenWork: local agent workbench. Rust workspace (5 crates in `crates/`) + Tauri 2 / React / TypeScript desktop app in `apps/desktop/`. PostgreSQL-only persistence via SQLx.

## Current state (read this first)

The workspace is **mid-change**, but everything currently compiles and all gates are green (run the postgres tests with `TEST_DATABASE_URL`, see below).

- `trace_spans` uses an explicit `trace_id` as its structural root; there is no `sequence` column and none should be reintroduced.
- `crates/openwork-core/migrations/` was **consolidated into a single initial schema**; the seven previous migrations were deleted. Any existing database must be recreated (`docker compose down -v`), because `_sqlx_migrations` still records versions whose files no longer exist.
- **Trace was re-scoped from ops instrumentation to quality tracking.** Payload capture, Desktop on-demand reads, runtime content policy, age-based retention, and candidate-scoped orphan sweeping are implemented. Still missing: everything about `trace_annotations` beyond its schema. **Cost was implemented and then removed by decision** — `docs/trace.md` §18 records why; do not reintroduce it.
- **The attribute types went from 78 fields to 55**; the two nested types (`ModelTransportAttemptTrace`, `CompactionAttemptRollup`) and all 13 content-shadow fields are gone. `docs/trace.md` §15 has the removal history and the three-question threshold any new attribute must pass.
- The target design is in `docs/`. Each doc's "尚未实施" section lists what has not converged yet. **Write new code to the docs, not to the current code.**

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
- Never edit an applied migration; add a new file: `sqlx migrate add --source crates/openwork-core/migrations <description>`. The one-off consolidation noted above is finished — do not delete or rewrite migrations again.
- Local reset (destroys all data): `docker compose down -v && docker compose up -d postgres && cargo run -p openwork-core --bin openwork-migrate`.
- Schema, time-field, and migration rules: [.claude/rules/database.md](.claude/rules/database.md). It also lists the places that do not yet comply — write new code to the rule, not to those.

## Architecture invariants

Authoritative design docs: `docs/` — one document per feature (see `docs/README.md` index). Source of truth for "what exists now" is the code + migrations.

- `openwork-core` is the **only** runtime entry. `OpenWorkCore` owns provider repository, credentials, and the session registry. Tauri (`apps/desktop/src-tauri`) only adapts Commands/Events and maps errors — no state machine duplication.
- Dependency direction (never reversed): `openwork-models` ← `openwork-tools`, `openwork-chat-state` ← `openwork-agent` ← `openwork-core` ← `apps/desktop/src-tauri`. `openwork-models` depends on no other OpenWork crate.
- One active session = one `SessionActor`; it drives at most one Turn at a time. The actor's run loop is the only place that advances the Model → Tool/Permission → Model chain. Trace, storage, and desktop never advance a Turn.
- `openwork-chat-state` is the single writer of the conversation; core writes via commands, reads via snapshots.
- `openwork-agent` holds only static agent definition (prompt, tool set, limits) — it must not start async run loops.
- Trace is **quality tracking**, not ops instrumentation: what the model saw, what it said, how many tokens it burned, how a human rated it. Writes stay best-effort — trace/queue/DB failures must not fail a Turn, and a payload write failure must still leave the span itself persisted.
- A new trace attribute must pass three questions: will anyone make a different decision because of it; can it be derived from other fields; is it null-or-constant on 99% of rows. A field costs six edits (Rust type, serialization whitelist, frontend whitelist, three locale files, with a test enforcing the last three) — and more importantly, a detail panel with 50 flat rows is not a panel. Per-attempt transport detail and any counter derivable from child spans are out by rule, not by preference.
- Trace's structural root is `trace_id` (no FK); `session_id`/`turn_id` are business labels and `turn_id` is nullable — manual compaction and rewind have no Turn. Any span kind that can lack a Turn must ship a read path with it, or it is write-only.
- **Trace never copies content that `messages` already holds.** Successful responses are referenced via `trace_spans.response_message_id`; tool input/output via `(turn_id, provider_call_id)`; successful summaries via `checkpointId`. Trace stores only what no other table has: the assembled request, System Context, tool definitions, and responses from calls that produced no Message. See `docs/trace.md` §6.
- **Trace records tokens, not money.** Cost was built and then removed — see `docs/trace.md` §18 for why. Do not reintroduce price columns, amount columns, or cost rollups without first fixing the state ambiguity described there.
- Token columns are **not comparable across providers**: whether `cached_input_tokens` is already inside `input_tokens` differs per vendor (`docs/trace.md` §7). Any aggregate must group by `resolved_provider_kind`.
- `trace_payloads` has no `session_id` (that is what makes dedup work), so deleting a session does **not** cascade its content away. The orphan sweep must run in the same transaction as the delete — it is a privacy requirement, not a space optimization. It sweeps only the hashes that session referenced, and shares a transaction-level advisory lock with payload attachment: `RESTRICT` guards references that already exist, the lock covers the window where a body is inserted but its mapping is not yet attached.
- Permission `Allow` cannot bypass `ToolSessionContext` path/process boundaries. There is no OS-level sandbox.
- Model is always explicitly chosen by the user (`providerId + model`); no auto-selection or cross-model fallback.

## Frontend ↔ Rust contract

- Contract codegen is deferred. Bridge DTOs are **hand-written** in `apps/desktop/src/bridge/compat.ts` and must be manually kept in sync with the Rust host DTOs (defined in `openwork-core`, e.g. `SessionInput` in `storage/postgres.rs`, surfaced via `src-tauri/src/commands/`). Bump `RUNTIME_SESSION_UPDATE_VERSION` in `compat.ts` when the update shape changes.
- React only talks to core through `apps/desktop/src/bridge/commands.ts` invoke wrappers — never SQL, provider adapters, or tool executors.
- Frontend state layers are separate: canonical session/messages, per-session runtime view (reducer), local UI state. See `docs/desktop.md`.

## Conventions

- Domain vocabulary: Session, Turn, Model Call, Tool Call, Permission, Message, Update, Trace, Compaction. Do not reintroduce retired terms (`StepId`, `ToolRunId`, `ApprovalId`, `JournalTurnRecorder`, Event Journal).
- Manual Conversation compaction remains an explicit `/compact` operation and is allowed only while the Session is idle. Before each provider submission, Core uses the application context-window capacity and a default 85% threshold for one pre-sampling compaction; an explicit `ContextOverflow` before semantic output may use the same one-compaction allowance. Do not add lossy fallback, fuzzy error-message matching, repeated compaction loops, or tool replay.
- Conversation checkpoints may be reconstructed after restart and selected for read-only replay or durable rewind. This recovers only the model-visible Conversation; it does not recover an unfinished Turn, draft, permission waiter, process, or tool side effect.
- Explicit V1 non-goals — do not add: MCP, Memory, Plan, Skill, Artifact, Git/Diff, Worktree, cross-process unfinished-Turn recovery, Event Journal, lossy compaction, or background-task recovery.
- Root README is bilingual: when editing `README.md`, mirror changes in `README.en.md`.
