<div align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/openwork-wordmark-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="docs/assets/openwork-wordmark-light.svg">
    <img src="docs/assets/openwork-wordmark-light.svg" alt="OpenWork" width="420">
  </picture>

  <p><strong>A local-first, traceable desktop agent workbench</strong></p>
  <p>A Rust runtime drives the Model → Tool/Permission → Model loop while Tauri Desktop manages models, sessions, tool permissions, and runtime traces.</p>

  <p>
    <img alt="Version" src="https://img.shields.io/badge/version-0.1.0-2563eb">
    <img alt="Status" src="https://img.shields.io/badge/status-active_development-f59e0b">
    <img alt="Rust" src="https://img.shields.io/badge/backend-Rust-dea584">
    <img alt="Tauri" src="https://img.shields.io/badge/desktop-Tauri_2-24c8db">
    <img alt="PostgreSQL" src="https://img.shields.io/badge/storage-PostgreSQL_16-4169e1">
  </p>

  <p>
    <a href="README.md">简体中文</a> ·
    <a href="docs/README.md">Documentation</a> ·
    <a href="docs/README.md">Architecture</a> ·
    <a href="https://github.com/Aaron-Ben/OpenWork/issues">Issues</a>
  </p>
</div>

> [!IMPORTANT]
> OpenWork `0.1.0` is under active development and currently runs from source. It enforces workspace and tool-permission boundaries, but it **does not provide an OS-level sandbox**. Use it only with trusted directories and acceptable permission settings.

## What is OpenWork?

OpenWork is a local desktop agent workbench. Choose a model and working directory, then let the agent read, search, and modify files, run commands, and pause for your permission decisions inside a persistent session.

The project focuses on a clear, diagnosable local runtime:

- **Multiple model providers**: built-in profiles for OpenAI, Anthropic, DeepSeek, Kimi, Qwen, and GLM;
- **Persistent agent loop**: one turn may contain multiple model and tool calls until it completes, fails, is cancelled, or reaches a safety guard;
- **Controlled tool execution**: built-in `read`, `write`, `edit`, `grep`, `glob`, `list`, and `bash` tools constrained by the working directory and permission profile;
- **Reviewable file changes**: file tools produce structured diffs with conflict-aware Undo/Reapply;
- **Local persistence**: providers, models, sessions, turns, messages, and traces live in PostgreSQL;
- **Runtime diagnostics**: Trace shows model/tool calls, actual retries, token usage, phase timing, permission waits, and capture completeness;
- **Desktop experience**: Tauri 2 + React with Simplified Chinese, Traditional Chinese, and English interfaces.

## Quick start

### 1. Prerequisites

- Rust stable
- Node.js with Corepack/pnpm
- Docker with Docker Compose
- The [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your platform

If pnpm is not installed:

```bash
corepack enable
corepack prepare pnpm@latest --activate
```

### 2. Clone and configure

```bash
git clone https://github.com/Aaron-Ben/OpenWork.git
cd OpenWork
cp .env.example .env
openssl rand -base64 32
```

Put the generated value in the root `.env` file:

```dotenv
DATABASE_URL=postgres://openwork:openwork@localhost:5432/openwork
OPENWORK_API_KEY_ENCRYPTION_KEY=<generated Base64 value>
```

> [!WARNING]
> Do not change `OPENWORK_API_KEY_ENCRYPTION_KEY` while keeping the same database. Existing provider API keys will become undecryptable.

### 3. Start PostgreSQL and migrate

Run from the repository root:

```bash
docker compose up -d postgres
cargo run -p openwork-core --bin openwork-migrate
```

### 4. Start Desktop

```bash
cd desktop
pnpm install
pnpm tauri dev
```

Desktop commands must run inside `desktop`; the repository root does not contain a `package.json`. `OpenWorkCore::bootstrap` also applies pending migrations. The standalone migration command is mainly useful for first-time provisioning and database diagnostics.

## Basic workflow

1. Configure a provider, model, and API key in Settings;
2. Create a session and explicitly choose `providerId + model` and a working directory;
3. Submit a task and allow or deny tool requests when prompted;
4. Review tool activity and file diffs, then Undo/Reapply changes when needed;
5. Open Trace from the runtime records or session UI to inspect retries, tool latency, and failure phases.

OpenWork never auto-selects a model and never silently falls back to a different model when a provider fails.

## Runtime architecture

```mermaid
flowchart LR
    UI["Tauri Desktop<br/>React + TypeScript"] -->|"Command / Event"| Core["OpenWorkCore"]
    Core --> Registry["Session Registry"]
    Registry --> Actor["SessionActor"]
    Actor --> Chat["Chat State Actor"]
    Actor --> Model["Model Adapters<br/>HTTP + SSE"]
    Actor --> Tools["Tool Runtime<br/>Permission + Workspace"]
    Core --> DB[("PostgreSQL")]
    Actor -. "best-effort signals" .-> Trace["Trace Recorder"]
    Trace --> DB
```

Key architecture invariants:

- `openwork-core` is the only runtime entry point;
- one active session maps to one `SessionActor`, which advances at most one turn at a time;
- `openwork-chat-state` is the only conversation writer;
- Desktop adapts Commands/Events and does not duplicate the backend state machine;
- Trace is best-effort diagnostics and cannot advance or alter a turn;
- unfinished turns become `interrupted` after process restart and tools are never replayed automatically.

## Repository layout

| Path | Responsibility |
| --- | --- |
| `desktop` | Tauri 2 / React / TypeScript desktop client |
| `crates/openwork-core` | Core facade, session runtime, PostgreSQL storage, and Trace |
| `crates/openwork-agent` | Agent definition, system prompt, and static policy |
| `crates/openwork-chat-state` | Single-writer conversation actor and model-request snapshots |
| `crates/openwork-models` | Model protocols, provider adapters, HTTP/SSE transport, and error classification |
| `crates/openwork-tools` | Tool catalog, permission policy, file/process execution, and structured file-change results |
| `docs` | Authoritative design docs, one per feature, each with its own acceptance checklist |

Dependencies remain one-way: `openwork-models` is the foundation; `openwork-tools` and `openwork-chat-state` depend on its model contracts; `openwork-agent` depends on the tool contract; `openwork-core` composes the runtime; and Tauri sits at the outer host boundary.

## Data and security boundaries

- Provider API keys are stored in PostgreSQL using AES-256-GCM, a random nonce, and a versioned envelope;
- the master key is injected only through `OPENWORK_API_KEY_ENCRYPTION_KEY` and is never stored in PostgreSQL;
- Debug Desktop builds load the root `.env`; release builds do not load the development `.env`;
- Permission `Allow` cannot bypass `ToolSessionContext` path and process boundaries;
- there is currently no OS-level sandbox, so the application process retains the access of its operating-system account;
- Trace V0.1 records allowlisted status, count, size, timing, and error fields by default—not full prompts, provider bodies, or raw tool input/output.

Low-level adapters can also use environment-backed configuration. Common variables:

```bash
OPENWORK_API_KEY_ENCRYPTION_KEY=...
OPENAI_API_KEY=...
ANTHROPIC_API_KEY=...
KIMI_API_KEY=...
DEEPSEEK_API_KEY=...
DASHSCOPE_API_KEY=...
```

Never commit real secrets.

## Development and verification

Run Rust commands from the repository root:

```bash
cargo test
cargo clippy --all-targets --all-features
cargo fmt
```

Run Desktop commands from `desktop`:

```bash
pnpm test
pnpm build
pnpm tauri build
```

PostgreSQL integration tests require an explicit test database; otherwise those tests return early:

```bash
TEST_DATABASE_URL=postgres://openwork:openwork@localhost:5432/openwork \
  cargo test -p openwork-core
```

## Current boundaries

Still incomplete:

- generated Rust → TypeScript host contracts and a drift check;
- a production Trace disable path and full queue/database/flush degradation verification;
- OS-level sandboxing and reliable tool side-effect reconciliation;
- automated live-provider smoke tests.

The current `0.1.x` scope supports explicit `/compact` on an idle Session and preflight compaction before each provider submission when the provider-neutral estimate reaches the default 85% of the application context-window budget. Threshold compaction and an explicit `ContextOverflow` before semantic output share a limit of one compaction per logical Model Call; fuzzy error matching, lossy input degradation, and tool replay remain excluded. A summary call is attempted at most three times with a timeout per attempt; successful compaction produces a versioned structured summary, a frozen runtime reminder, and a reconstructible Conversation checkpoint. PostgreSQL reconstructs the latest Conversation after restart, supports read-only checkpoint replay and durable rewind, and exposes checkpoint-bounded raw-message pagination through the Host API and the read-only `conversation_history` tool; readback never executes a tool. This is not unfinished-Turn recovery. Failure classification and automatic suppression, cross-process unfinished-Turn recovery, an Event Journal, memory, MCP, planning, skills, Git integration, repository-level diffs, worktrees, and background-task recovery remain out of scope. The authoritative design documents define the complete boundary.

## Documentation

- [Documentation index](docs/README.md)
- [Documentation index](docs/README.md)
- [Architecture and dependency direction](docs/architecture.md)
- [Session runtime](docs/session-runtime.md)
- [Context window layering](docs/context-window.md)
- [Compaction](docs/compaction.md)
- [Trace](docs/trace.md)
- [Tools](docs/tools.md)
- [Data model](docs/data-model.md)
- [Desktop](docs/desktop.md)
- [Local PostgreSQL and SQLx migrations](docs/local-postgres.md)

Found a bug or want to discuss the design? Open a [GitHub Issue](https://github.com/Aaron-Ben/OpenWork/issues).
