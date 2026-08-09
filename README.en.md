<div align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/openwork-wordmark-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="docs/assets/openwork-wordmark-light.svg">
    <img src="docs/assets/openwork-wordmark-light.svg" alt="OpenWork" width="420">
  </picture>

  <p><strong>An auditable local desktop agent workbench</strong></p>
  <p>Manage sessions, tool execution, and traces locally; call the model provider you explicitly choose;<br>and complete code and file tasks inside a selected directory.</p>

  <p>
    <img alt="Target" src="https://img.shields.io/badge/target-0.1.0-2563eb">
    <img alt="Status" src="https://img.shields.io/badge/status-active_development-f59e0b">
    <img alt="License" src="https://img.shields.io/badge/license-Apache--2.0-0f766e">
    <img alt="Rust" src="https://img.shields.io/badge/backend-Rust-dea584">
    <img alt="Tauri" src="https://img.shields.io/badge/desktop-Tauri_2-24c8db">
    <img alt="PostgreSQL" src="https://img.shields.io/badge/storage-PostgreSQL_16-4169e1">
  </p>

  <p>
    <a href="README.md">简体中文</a> ·
    <a href="docs/README.md">Documentation</a> ·
    <a href="docs/architecture.md">Architecture</a> ·
    <a href="https://github.com/Aaron-Ben/OpenWork/issues">Issues</a>
  </p>
</div>

> [!IMPORTANT]
> OpenWork has not been released yet. It is being developed toward `0.1.0` and currently runs from source only. `bash` runs on the host with the current operating-system user's permissions; permission rules and approvals are not an OS sandbox. Use OpenWork only on trusted projects, and understand its auto-approval rules and host access.

## What is OpenWork?

OpenWork is a local desktop agent workbench. After you choose a model and working directory, a persistent agent loop advances the Model → Tool/Permission → Model chain within one turn until the task completes, fails, is cancelled, or reaches a safety guard.

The application runtime, sessions, messages, and traces stay on your machine; model inference requests are sent to the provider you configure. OpenWork is not an offline model runner, and it never chooses or switches models on your behalf.

## Current capabilities

- **Explicit model selection**: built-in presets for OpenAI, Anthropic, DeepSeek, Kimi, Qwen, and GLM. The user selects the concrete model for each session; there is no silent cross-model fallback;
- **Seven built-in tools**: `read`, `write`, `edit`, `grep`, `glob`, `list`, and `bash`. The first six resolve real paths and verify authorization through one shared boundary; `bash` starts the host POSIX shell in the working directory;
- **Two permission modes**: `default` auto-allows workspace reads and commands proven to be read-only; `acceptEdits` additionally allows non-sensitive workspace file changes. Commands that cannot be proven safe still require confirmation;
- **Reviewable file changes**: `write` and `edit` produce structured diffs with conflict-aware Undo / Reapply. File changes made through `bash` do not yet have equally reliable reconciliation;
- **Inspectable context**: `AGENTS.md` at the working-directory root enters the system context, and Desktop can show the composition and budget of the next model request;
- **Context compaction**: compact automatically near the context limit or explicitly run `/compact` while a session is idle. Summaries, checkpoints, read-only replay, and durable rewind are persisted;
- **Quality traces**: inspect the request actually sent to the model, system context, tool definitions, response references, tokens, permission decisions, and failure phases;
- **Local persistence**: providers, credentials, sessions, turns, messages, compactions, and traces use PostgreSQL. API keys are encrypted with AES-256-GCM;
- **Three interface languages**: Simplified Chinese, Traditional Chinese, and English.

## What makes it auditable

| What you want to verify | Evidence OpenWork currently provides |
|---|---|
| What the model actually saw | Trace stores the assembled request, system context, and tool definitions that were submitted—not an after-the-fact guess |
| Why a tool call was allowed or required approval | Tool-call traces record the permission mode, decision source, rule, or read-only proof |
| What `write` / `edit` changed | Tool results carry before/after hashes and structured diffs, with Undo / Reapply |
| What remains after a long conversation is compacted | A checkpoint stores the summary, factual boundary, and runtime reminder; compaction does not delete original messages |
| Where time or tokens went, and why a call failed | Model, tool, and compaction spans record phase timing, failure phase, and tokens |

Trace is evidence for diagnosis and quality review. It does not advance a turn, infer unknown tool side effects, or authorize automatic tool replay.

## Quick start

Requirements:

- Rust stable;
- Node.js and pnpm;
- Docker with Docker Compose;
- the [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your platform.

Clone the repository and prepare the environment:

```bash
git clone https://github.com/Aaron-Ben/OpenWork.git
cd OpenWork
cp .env.example .env
openssl rand -base64 32
```

Put the output of the last command in the root `.env` file:

```dotenv
DATABASE_URL=postgres://openwork:openwork@localhost:5432/openwork
OPENWORK_API_KEY_ENCRYPTION_KEY=<generated Base64 value>
```

> [!WARNING]
> Do not change `OPENWORK_API_KEY_ENCRYPTION_KEY` while keeping the same database. Existing provider API keys will become undecryptable.

Start PostgreSQL, apply migrations, and launch Desktop:

```bash
docker compose up -d postgres
cargo run -p openwork-core --bin openwork-migrate
cd desktop
pnpm install
pnpm tauri dev
```

Desktop commands must run inside `desktop/`; the repository root has no `package.json`. Debug Desktop loads the root `.env`, while release builds do not load the development `.env`.

Once running, configure a provider and API key in Settings, create a session with a model and working directory, and submit a task. Permission requests pause the current turn when needed; structured file-tool changes and traces remain available for review in the session UI.

## Permissions and security boundaries

| Boundary | Current behavior |
|---|---|
| Six file tools | Resolve and validate the real target or parent through one boundary. Workspace paths cannot silently escape through relative paths or symlinks; explicit outside-workspace access needs an execution permit for that call |
| `default` | Automatically reads workspace files and runs commands proven read-only by a closed allowlist; ordinary writes and other commands require confirmation |
| `acceptEdits` | Additionally allows non-sensitive workspace `write` / `edit` calls plus restricted filesystem-command forms and output redirection; other commands still require confirmation |
| `bash` | Syntax analysis informs the permission decision but is not runtime isolation; approving `bash` means trusting the command and the programs it starts |
| Network | Not enforced; host processes retain the network capabilities of the operating-system account |
| Credentials | API keys are encrypted in PostgreSQL; the master key is injected from the environment and is never stored in the database |
| Trace payloads | Always record every supported payload slot and may contain private code and model requests; the default retention period is 30 days |

OpenWork **deliberately does not provide OS-level sandboxing, network control, unattended execution, or a “run any command without asking” mode**. See the [permission design](docs/permissions.md) for the complete rationale and semantics.

## Not yet implemented

The development build targeting `0.1.0` does not yet implement MCP, memory, planning, skills, subagents, an independent artifact subsystem, Git or repository-level diffs, worktrees, cross-process recovery of unfinished turns, an event journal, lossy compaction, or background-task recovery. This describes the current state; it does not commit every capability to a future release. After a process restart, unfinished turns are marked `interrupted`; tools are never replayed automatically.

The following are known engineering gaps, not commitments that every item is in the current iteration:

- the Rust → TypeScript host contract is still mirrored by hand, with no generation or drift check;
- trace annotations still have schema only, while full queue/database/flush degradation verification and an outlet for dropped-write counters remain incomplete;
- file side effects caused by `bash` cannot yet be reconciled reliably;
- a manual live-provider smoke-test entry point exists, but it is not automated.

Source code is the authority for current behavior; `docs/` describes the target design. When the two differ, each design document's “not yet implemented” section identifies the gap.

## For contributors

```mermaid
flowchart LR
    UI["Tauri Desktop<br/>React + TypeScript"] -->|"Command / Event"| Core["OpenWorkCore"]
    Core --> Registry["Session Registry"]
    Registry --> Actor["SessionActor"]
    Actor --> Chat["Chat State Actor"]
    Actor --> Model["Model Adapters<br/>HTTP + SSE"]
    Actor --> Tools["Tool Runtime<br/>Permission + Workspace"]
    Core --> DB[("PostgreSQL")]
    Actor -. "best-effort" .-> Trace["Trace Recorder"]
    Trace --> DB
```

| Path | Responsibility |
|---|---|
| `desktop/` | Tauri 2 / React client; adapts Commands / Events and never duplicates the backend state machine |
| `crates/openwork-core/` | The only runtime entry point: session runtime, agent loop, compaction, storage, and Trace |
| `crates/openwork-agent/` | The agent's static definition: system prompt, toolset, and limits |
| `crates/openwork-chat-state/` | The only writer of the conversation |
| `crates/openwork-models/` | Model protocols, provider adapters, HTTP / SSE, and error classification |
| `crates/openwork-tools/` | Tool contracts, permission policy, path safety, and file/process backends |
| `docs/` | Authoritative design documents, one per feature |

Dependencies remain one-way: `openwork-models` is at the bottom, `openwork-core` composes the runtime, and Tauri sits at the outer boundary. Three core invariants are: one `SessionActor` per active session, one agent loop in `session/run_loop.rs`, and a Trace failure must never change a business result. See the [architecture document](docs/architecture.md) for the complete constraints.

Verification commands:

```bash
# repository root
cargo test
cargo clippy --all-targets --all-features
cargo fmt

# desktop/
pnpm test
pnpm build
```

PostgreSQL integration tests require an explicit test database; otherwise those tests return early:

```bash
TEST_DATABASE_URL=postgres://openwork:openwork@localhost:5432/openwork \
  cargo test -p openwork-core
```

Documentation: [index](docs/README.md) · [architecture](docs/architecture.md) · [session runtime](docs/session-runtime.md) · [context window](docs/context-window.md) · [compaction](docs/compaction.md) · [Trace](docs/trace.md) · [tools](docs/tools.md) · [permissions](docs/permissions.md) · [data model](docs/data-model.md) · [Desktop](docs/desktop.md) · [local PostgreSQL](docs/local-postgres.md)

## License

OpenWork is licensed under the [Apache License 2.0](LICENSE).

---

Found a bug or want to discuss the design? Open a [GitHub Issue](https://github.com/Aaron-Ben/OpenWork/issues).
