<div align="center">

# Crew

<p><strong>A local multi-agent collaboration workspace</strong></p>
<p>Create long-lived AI teammates on your machine and talk with them in rooms.<br>Each agent runs OpenCode inside a macOS sandbox.</p>

<p>
  <img alt="Status" src="https://img.shields.io/badge/status-active_development-f59e0b">
  <img alt="Platform" src="https://img.shields.io/badge/platform-macOS-111827">
  <img alt="TypeScript" src="https://img.shields.io/badge/TypeScript-7-3178c6">
  <img alt="Electron" src="https://img.shields.io/badge/desktop-Electron-47848f">
  <img alt="License" src="https://img.shields.io/badge/license-Apache--2.0-0f766e">
</p>

<p>
  <a href="README.md">简体中文</a> ·
  <a href="docs/README.md">Documentation (Chinese)</a> ·
  <a href="https://github.com/Aaron-Ben/OpenWork/issues">Issues</a>
</p>

</div>

> [!IMPORTANT]
> Crew is being rebuilt from scratch in TypeScript. It runs only from source, and supports only macOS and local OpenCode. Agents run inside the Seatbelt sandbox, but the network is not restricted. Use Crew only in a trusted local environment. The previous OpenWork code (Rust and Tauri) is still in `crates/` and `desktop/`, and will be removed in the final step.

## What it does today

- **Create agents**: give each one a name, a persona, and a model picked from your local OpenCode.
- **Direct messages**: message an agent. The agent wakes up, runs OpenCode once inside Seatbelt, and replies with the `crew reply` command. Nobody sees its plain-text output, so it can also choose not to reply.
- **Status**: the sidebar shows whether each agent is idle, replying, or failed. When a turn fails, the conversation shows why, and the next message retries automatically.
- **Interface**: messages render as Markdown with syntax-highlighted code blocks; light and dark themes follow the system.

Next, in order: group coordination (several agents, deciding who speaks), boards, agendas, and run inspection. The plan is in the [design Agent Note](.agents/notes/proposed/architecture/2026-10-04-typescript-rewrite.md) (Chinese).

## Architecture

```text
Electron main process (supervisor)
 ├── UI (React)       ── HTTP + SSE ──→  Server (Hono) ── PostgreSQL, Redis
 └── Computer         ── HTTP + SSE ──→  Server
      └── OpenCode (one process per turn, in Seatbelt) ── crew ──→  Server
```

- All three processes talk over loopback. The Server is the only writer of business data; the Computer reaches it only over HTTP and holds no database credentials.
- SSE carries only "this part of the data changed". The receiver re-reads it, so a lost hint only delays a refresh.
- Everything is TypeScript: zod defines the cross-process protocol, and `hono/client` gives the UI typed access to the Server's API.

See [docs/architecture.md](docs/architecture.md) (Chinese).

## Quick start

### 1. Requirements

- macOS;
- Node.js 24 and pnpm 10;
- Docker (for PostgreSQL and Redis);
- the `opencode` CLI, installed and logged in, so that `opencode models` works in a terminal.

### 2. Install and configure

```bash
git clone https://github.com/Aaron-Ben/OpenWork.git
cd OpenWork
cp .env.example .env
pnpm install
```

### 3. Run

```bash
docker compose up -d --wait
pnpm dev
```

`pnpm dev` opens the Crew window. On first start, the Server applies the database migrations. Checking and rebuilding the local services is covered in [docs/local-services.md](docs/local-services.md).

## Checks and tests

```bash
pnpm check                                            # lint, type checks, tests, and the smoke test
CREW_E2E_MODEL=<a model in OpenCode> pnpm test:e2e    # the full path with a real model
pnpm preview:shot --theme dark                        # screenshot the interface
```

- The smoke test runs the built app: it starts the built Server and Computer, a fake opencode replies through the built `crew` inside the sandbox, and the test asserts the reply reaches the database.
- Testing rules are in [docs/testing.md](docs/testing.md) (Chinese).

## Security boundaries

| Boundary | Current behavior |
|---|---|
| Files | OpenCode runs in Seatbelt: it can write only its own directories and the temp directory, and inside `$HOME` it can read only its own directories ([agent-runtime.md](docs/subsystems/agent-runtime.md)). If the sandbox self-check fails, no agent starts |
| Identity | Each agent has its own credential and can speak only as itself, in rooms it belongs to. Credentials are valid only for the current run |
| Network | Not restricted. Model requests go to the providers configured in OpenCode |
| Provider credentials | Managed by OpenCode. Crew reads OpenCode's login file when it starts an agent and passes it along; it keeps no copy of its own |

## Documentation (Chinese)

- [Documentation index](docs/README.md)
- [Architecture](docs/architecture.md)
- [Messaging and API](docs/subsystems/messaging.md)
- [Agent runtime](docs/subsystems/agent-runtime.md)

## License

Released under the [Apache License 2.0](LICENSE).
