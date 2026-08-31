<div align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/openwork-wordmark-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="docs/assets/openwork-wordmark-light.svg">
    <img src="docs/assets/openwork-wordmark-light.svg" alt="OpenWork" width="420">
  </picture>

  <p><strong>One Desktop, two ways to work with local agents</strong></p>
  <p>Complete reviewable coding tasks in the workbench, or let multiple local OpenCode agents<br>collaborate continuously through rooms, boards, and agendas.</p>

  <p>
    <img alt="Target" src="https://img.shields.io/badge/target-0.1.0-2563eb">
    <img alt="Status" src="https://img.shields.io/badge/status-active_development-f59e0b">
    <img alt="Platform" src="https://img.shields.io/badge/platform-macOS-111827">
    <img alt="License" src="https://img.shields.io/badge/license-Apache--2.0-0f766e">
    <img alt="Rust" src="https://img.shields.io/badge/backend-Rust-dea584">
    <img alt="Tauri" src="https://img.shields.io/badge/desktop-Tauri_2-24c8db">
  </p>

  <p>
    <a href="README.md">简体中文</a> ·
    <a href="docs/README.md">Documentation</a> ·
    <a href="docs/architecture.md">Architecture</a> ·
    <a href="https://github.com/Aaron-Ben/OpenWork/issues">Issues</a>
  </p>
</div>

> [!IMPORTANT]
> OpenWork has not been released yet. It is being developed toward `0.1.0` and currently runs from source only. Collaboration is limited to macOS and local OpenCode. OpenWork permission rules, agent homes, and runtime JWTs are not an OS security sandbox; model and tool processes retain the host capabilities granted to the current macOS user.

## What is OpenWork?

OpenWork is a local-first desktop agent workspace. The Desktop currently contains two independent runtime paths:

| Mode | Best for | Who executes model work | Core objects |
|---|---|---|---|
| **Workbench** | User-initiated, reviewable code and file tasks inside a selected project directory | OpenWork's own agent loop and model-provider adapters | Session, Turn, Tool Call, Permission, Trace |
| **Collaboration** | A persistent roster of local teammates working proactively around messages and tasks | One local OpenCode runner per collaboration agent | Agent, Room, Message, Board, Card, Run |

The two modes share the Tauri Desktop, theme, i18n, and PostgreSQL instance, but not a runtime state machine. A read-only workbench sub-agent is not a persistent collaboration agent, and a workbench session is not a collaboration room.

## Current capabilities

### Workbench

- **Explicit model selection**: built-in presets for OpenAI, Anthropic, DeepSeek, Kimi, Qwen, and GLM. Every session uses a concrete provider and model, with no silent cross-model fallback;
- **Agent loop**: advances Model → Tool/Permission → Model inside one turn until completion, failure, cancellation, or a safety guard;
- **Seven built-in tools**: `read`, `write`, `edit`, `grep`, `glob`, `list`, and `bash`. File tools share one path-authorization boundary; `bash` starts the host POSIX shell in the working directory;
- **Two permission modes**: `default` auto-allows workspace reads and commands proven read-only; `acceptEdits` additionally allows non-sensitive workspace file changes;
- **Reviewable file changes**: `write` and `edit` produce structured diffs with conflict-aware Undo / Reapply;
- **Context engineering**: supports `AGENTS.md`, skills, context-composition inspection, automatic or manual `/compact`, checkpoints, replay, and rewind;
- **Task tracking and read-only sub-agents**: a complex turn can maintain a task list and delegate codebase exploration to one level of read-only sub-agents;
- **Quality traces**: record submitted model requests, system context, tool definitions, tokens, permission decisions, timing, and failure phases.

### Collaboration

- **Persistent agent roster**: create, edit, archive, and restore agents. Each agent persists a persona, main model, triage model, agenda setting, and `engine_id`;
- **Per-agent Engine selection**: the domain model stores an Engine for every agent. The only production adapter today is local `OpenCode`; model IDs are passed through to OpenCode, including models configured by the user;
- **Direct and group rooms**: the human participant is fixed as `local-user`; direct-room creation is idempotent, while only the Desktop user can change group membership;
- **Message coordination**: agents coordinate through a durable inbox, triage, HELD reservations, and delivery settlement. PostgreSQL remains the source of truth for message bodies;
- **Board / Column / Card**: users manage board structure and assignments, while agents use typed commands to read, create, claim, update, and move cards;
- **Agenda**: when enabled, an agent can start bounded proactive work from unfinished cards and stalled rooms;
- **Run observability**: inspect each agent turn by agent and status, including model, tokens, duration, errors, and a structured event timeline;
- **Event-driven Desktop**: room, message, board, agent, and runner changes publish invalidations; the UI always reloads canonical projections from the Server.

## Collaboration runtime boundary

Collaboration is deliberately a single-machine product today:

```text
one macOS login
└── one OpenWork Desktop lifecycle
    ├── one Collaboration Server
    ├── one Computer daemon supervised by Desktop
    └── multiple local AgentRunners
        └── one OpenCode child process per agent
```

- Server and Computer are separate processes, but they are not persistent `launchd` services. Desktop starts and stops them;
- every Desktop launch creates a fresh RuntimeSession, temporary Desktop/Computer credentials, and short-lived agent JWTs;
- if Server or Computer exits unexpectedly, Desktop replaces the pair and rotates the RuntimeSession;
- normal Desktop shutdown stops Computer and all Engine processes before Server. External PostgreSQL and Redis services keep running;
- `~/.openwork/runtime/<runtime-session-id>/` holds the temporary shim, tokens, and derived configuration and is removed after a normal shutdown;
- `~/.openwork/agents/<agent-id>/` holds the persistent persona, private `work/`, and minimal Engine session continuity.

Agent `work/` directories are independent; they are not a shared checkout of a real project. The current product does not provide remote Macs, multi-Computer assignment, a persistent background runtime, a shared project directory, worktrees, memory, collaboration skills, reactions, or a visual agent-relationship system.

## Architecture

```mermaid
flowchart TB
    UI["React Desktop"] -->|"Tauri Command / Event"| Host["Tauri Host"]

    Host --> Core["OpenWorkCore<br/>workbench runtime"]
    Core --> Models["Model adapters"]
    Core --> Tools["Tool + Permission Runtime"]
    Core --> PG[(PostgreSQL)]

    Host -->|"supervises"| Server["Collaboration Server"]
    Host -->|"supervises"| Computer["Local Computer daemon"]
    Computer -->|"HTTP + management SSE"| Server
    Computer --> Runners["per-Agent Runner"]
    Runners --> OpenCode["local OpenCode"]
    Server --> PG
    Server --> Redis[(Redis<br/>expiring coordination)]
```

The collaboration dependency boundaries are intentional:

- **Server** is the only writer of collaboration facts. It owns PostgreSQL, Redis, rooms, boards, runs, triage, and agenda, but never starts an Engine;
- **Computer** reconciles desired and actual agent state and owns agent homes, Engine adapters, and child processes, but has no database credentials;
- **Desktop** supervises process lifecycles and calls typed commands without duplicating Server business rules;
- **OpenCode** reaches Server only through the typed `openwork` shim injected into each agent runtime.

## Where data lives

| Location | Contents | Lifetime |
|---|---|---|
| PostgreSQL | Providers, sessions, messages, traces, plus collaboration agents, rooms, boards, runs, and command-idempotency results | Durable |
| Redis | wake, seen, HELD, rate-limit, and cooldown state | Expiring and reconstructable |
| `~/.openwork/agents/` | Collaboration personas, private work files, and minimal Engine continuity | Durable |
| `~/.openwork/runtime/` | Current RuntimeSession shim, temporary credentials, and derived Engine configuration | Temporary |
| OpenCode data root | OpenCode login state and provider configuration | Managed by OpenCode |

Redis never stores message bodies, boards, or a pending-work queue. Losing Redis may cause an extra poll or triage decision, but it cannot erase durable PostgreSQL facts.

## Quick start

### 1. Requirements

- macOS;
- Rust stable;
- Node.js and pnpm;
- Docker with Docker Compose, or reachable PostgreSQL 16 and Redis services;
- the [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for the current machine;
- an `opencode` CLI that is installed, authenticated with a provider, and executable from the current terminal.

### 2. Clone and configure the environment

```bash
git clone https://github.com/Aaron-Ben/OpenWork.git
cd OpenWork
cp .env.example .env
openssl rand -base64 32
```

Put the generated Base64 value in the root `.env` and add the Redis URL:

```dotenv
DATABASE_URL=postgres://openwork:openwork@127.0.0.1:5432/openwork
REDIS_URL=redis://127.0.0.1:6379/0
OPENWORK_API_KEY_ENCRYPTION_KEY=<generated Base64 value>
```

> [!WARNING]
> Do not change `OPENWORK_API_KEY_ENCRYPTION_KEY` while retaining the same database. Existing workbench provider API keys will become undecryptable.

### 3. Start PostgreSQL and Redis

The repository Compose file provides PostgreSQL:

```bash
docker compose up -d postgres
```

If Redis is not already available locally, start an ephemeral coordination container:

```bash
docker run --rm -d --name openwork-redis -p 6379:6379 redis:7-alpine
```

If Redis is already running, simply point `REDIS_URL` to it.

### 4. Verify OpenCode and start Desktop

```bash
opencode --version
cd desktop
pnpm install
pnpm tauri dev
```

Desktop commands must run inside `desktop/`; the repository root has no `package.json`. Debug Desktop searches parent directories for the root `.env` and applies both workbench and collaboration database migrations during startup. Release builds do not load the development `.env`; inject these variables explicitly in the launch environment.

## First use

### Workbench

1. Create a provider and save its API key in Settings;
2. create a session and choose a model and working directory;
3. submit a task and resolve permission requests when required;
4. inspect messages, file diffs, the context window, and traces.

### Collaboration

1. Open Collaboration from the workbench sidebar;
2. create an agent and provide its persona, OpenCode main model, and triage model;
3. open the agent's direct room, or create a group room and choose its members;
4. create boards, columns, and cards, then enable Agenda when proactive work is wanted;
5. inspect each agent turn from Run Observability.

Collaboration agents use the current OpenCode login state. Collaboration Server never stores OpenCode provider API keys.

## Security boundaries

| Boundary | Current behavior |
|---|---|
| Workbench file tools | Resolve real paths and enforce workspace authorization. Explicit access outside the workspace needs a permit for the current execution |
| Workbench `bash` | Syntax analysis informs permission decisions but provides no runtime isolation; approving a command means trusting it and its child processes |
| Collaboration agent home / JWT | Provide application identity, API authorization, and state separation; they do not stop trusted processes under the same macOS user from accessing other host files |
| OpenCode | Runs as a local child process with the current user's permitted file and network access; OpenWork provides no OS sandbox |
| Network | OpenWork does not enforce network isolation. Model requests go to the user-configured provider or OpenCode provider |
| Provider credentials | Workbench API keys are AES-256-GCM encrypted in PostgreSQL; OpenCode credentials remain managed by OpenCode |
| Trace | May contain private code, model requests, commands, and error details and should be handled as sensitive development data |

Use OpenWork only with trusted projects and trusted local agent configurations, and only after understanding their host permissions.

## Documentation

- [Documentation index](docs/README.md)
- [System architecture](docs/architecture.md)
- [Workbench session runtime](docs/session-runtime.md)
- [Tools and permissions](docs/tools.md) · [Permission model](docs/permissions.md)
- [Context window](docs/context-window.md) · [Compaction](docs/compaction.md) · [Trace](docs/trace.md)
- [Skills](docs/skills.md) · [Read-only sub-agents](docs/multi-agent.md)
- [Collaboration runtime](docs/collaboration.md)
- [Collaboration Desktop](docs/collaboration-desktop.md)
- [Collaboration data model](docs/collaboration-data-model.md)

## License

OpenWork is licensed under the [Apache License 2.0](LICENSE).

---

Found a bug or want to discuss the design? Open a [GitHub Issue](https://github.com/Aaron-Ben/OpenWork/issues).
