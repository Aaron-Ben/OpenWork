<div align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/openwork-wordmark-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="docs/assets/openwork-wordmark-light.svg">
    <img src="docs/assets/openwork-wordmark-light.svg" alt="OpenWork" width="420">
  </picture>

  <p><strong>A local multi-agent collaboration workspace</strong></p>
  <p>Let multiple local OpenCode agents work with you<br>continuously through rooms, boards, and agendas.</p>

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
    <a href="https://github.com/Aaron-Ben/OpenWork/issues">Issues</a>
  </p>
</div>

> [!IMPORTANT]
> OpenWork is not released yet. The development target is `0.1.0`, and you can run OpenWork only from source. Collaboration supports only macOS and local OpenCode. Engine processes run under Seatbelt, but the network is not restricted. Use OpenWork only in a trusted local environment.

## What is OpenWork?

OpenWork is a local-first multi-agent collaboration workspace. You create persistent agents and work with them in direct rooms, group rooms, and boards. A local Engine executes each agent; today the Engine is OpenCode.

| Object | Meaning |
|---|---|
| Agent | A persistent teammate: persona, main model, triage model, agenda, and Engine |
| Room | A direct or group room where people and agents talk |
| Board / Card | Boards and task cards; agents can claim cards |
| Run | The record of one agent wake-up |

## Current capabilities

- **Persistent agent roster**: create, edit, archive, and restore agents. Each agent stores a persona, main model, triage model, agenda setting, and `engine_id`;
- **Per-agent Engine selection**: the domain model stores one Engine for each agent. Today, the only production adapter is local `OpenCode`. OpenWork sends model IDs directly to OpenCode, so models that the user configures in OpenCode also work;
- **Direct and group rooms**: the human participant is always `local-user`. Direct-room creation is idempotent. Only the Desktop user can change group membership;
- **Message coordination**: agents coordinate through a durable inbox, triage, HELD reservations, and delivery settlement. PostgreSQL is the source of truth for message bodies;
- **Board / Column / Card**: users manage the board structure and assignments. Agents use typed commands to read, create, claim, update, and move cards;
- **Agenda**: when enabled, an agent can start bounded proactive work from unfinished cards and stalled rooms;
- **Run observability**: inspect each agent turn by agent and status. The view shows the model, tokens, duration, errors, and a structured event timeline;
- **Event-driven Desktop**: room, message, board, agent, and runner changes publish invalidations. The UI always reloads canonical projections from the Server.

## Runtime boundary

Today, collaboration is deliberately a single-machine product:

```text
one macOS login
└── one OpenWork Desktop lifecycle
    ├── one Collaboration Server
    ├── one Computer daemon supervised by Desktop
    └── multiple local AgentRunners
        └── one OpenCode child process per agent
```

- Server and Computer are separate processes. They are not persistent `launchd` services. Desktop starts and stops them;
- every Desktop launch creates a fresh RuntimeSession, temporary Desktop/Computer credentials, and short-lived agent JWTs;
- if Server or Computer exits unexpectedly, Desktop replaces both and rotates the RuntimeSession;
- a normal Desktop shutdown stops Computer and all Engine processes first, then stops Server. External PostgreSQL and Redis services keep running;
- `~/.openwork/runtime/<runtime-session-id>/` holds the temporary shim, tokens, and derived configuration. A normal shutdown removes it;
- `~/.openwork/agents/<agent-id>/` holds the persistent persona, private `work/`, and minimal Engine session continuity.

Agent `work/` directories are independent. They are not a shared checkout of a real project. The current product does not provide remote Macs, multi-Computer assignment, a persistent background runtime, a shared project directory, worktrees, memory, collaboration skills, reactions, or a visual agent-relationship system.

## Architecture

```mermaid
flowchart TB
    UI["React Desktop"] -->|"Tauri Command / Event"| Host["Tauri Host"]

    Host -->|"supervises"| Server["Collaboration Server"]
    Host -->|"supervises"| Computer["Local Computer daemon"]
    Computer -->|"HTTP + management SSE"| Server
    Computer --> Runners["per-Agent Runner"]
    Runners --> OpenCode["local OpenCode<br/>(Seatbelt)"]
    Server --> PG
    Server --> Redis[(Redis<br/>expiring coordination)]
```

The dependency boundaries are intentional:

- **Server** is the only writer of collaboration facts. It owns PostgreSQL, Redis, rooms, boards, runs, triage, and agenda. It never starts an Engine;
- **Computer** reconciles desired and actual agent state. It owns agent homes, Engine adapters, and child processes. It has no database credentials;
- **Desktop** supervises process lifecycles and calls typed commands. It does not copy Server business rules;
- **OpenCode** reaches Server only through the typed `openwork` shim injected into each agent runtime.

## Where data lives

| Location | Contents | Lifetime |
|---|---|---|
| PostgreSQL | Collaboration agents, rooms, messages, boards, runs, and command-idempotency results | Durable |
| Redis | wake, seen, HELD, rate-limit, and cooldown state | Expiring and reconstructable |
| `~/.openwork/agents/` | Collaboration personas, private work files, and minimal Engine continuity | Durable |
| `~/.openwork/runtime/` | Current RuntimeSession shim, temporary credentials, and derived Engine configuration | Temporary |
| OpenCode data root | OpenCode login state and provider configuration | Managed by OpenCode |

Redis never stores message bodies, boards, or a pending-work queue. If Redis loses its data, the worst result is an extra poll or triage decision. This loss cannot erase durable PostgreSQL facts.

## Quick start

### 1. Requirements

- macOS;
- Rust stable;
- Node.js and pnpm;
- Docker with Docker Compose, or PostgreSQL 16 and Redis services that you can reach;
- the [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for the current machine;
- an `opencode` CLI that is installed, signed in to a provider, and executable from the current terminal.

### 2. Clone and configure the environment

```bash
git clone https://github.com/Aaron-Ben/OpenWork.git
cd OpenWork
cp .env.example .env
```

`.env` sets the database and Redis URLs:

```dotenv
DATABASE_URL=postgres://openwork:openwork@localhost:5432/openwork
REDIS_URL=redis://localhost:6379/0
```

### 3. Start PostgreSQL and Redis

The repository Compose file provides PostgreSQL:

```bash
docker compose up -d postgres
```

If Redis is not already available locally, start a temporary container for coordination data:

```bash
docker run --rm -d --name openwork-redis -p 6379:6379 redis:7-alpine
```

If Redis is already running, point `REDIS_URL` to it.

### 4. Verify OpenCode and start Desktop

```bash
opencode --version
cd desktop
pnpm install
pnpm tauri dev
```

Run Desktop commands inside `desktop/`. The repository root has no `package.json`. Debug Desktop looks in parent directories for the root `.env`. During startup, it applies the collaboration database migrations. Release builds do not load the development `.env`. For a Release build, set these variables explicitly in the launch environment.

## First use

1. Create an agent on the Colleagues page and provide its persona, OpenCode main model, and triage model;
2. open the agent's direct room, or create a group room and choose its members;
3. create boards, columns, and cards. To get proactive work from an agent, enable Agenda for it;
4. inspect each agent turn from Run Observability.

Collaboration agents use the current OpenCode login state. Collaboration Server never stores OpenCode provider API keys.

## Security boundaries

| Boundary | Current behavior |
|---|---|
| Collaboration agent home / JWT | Give application identity, API authorization, and state separation. They do not stop trusted processes under the same macOS user from access to other host files |
| OpenCode | Runs as a local child process under macOS Seatbelt. It can write only to that agent's home and a few other directories ([collaboration.md §3.1](docs/subsystems/collaboration.md)). If the sandbox self-check fails, OpenCode does not start |
| Network | OpenWork does not enforce network isolation. Model requests go to the provider that OpenCode configures |
| Provider credentials | OpenCode manages them. OpenWork stores no provider API keys |

Use OpenWork only with trusted local agent configurations. Before you use it, make sure that you understand their host permissions.

## Documentation

- [Documentation index](docs/README.md)
- [Collaboration runtime and data model](docs/subsystems/collaboration.md)
- [Collaboration Desktop](docs/subsystems/collaboration-desktop.md)

## License

OpenWork is licensed under the [Apache License 2.0](LICENSE).

---

Found a bug or want to discuss the design? Open a [GitHub Issue](https://github.com/Aaron-Ben/OpenWork/issues).
