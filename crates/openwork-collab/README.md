# OpenWork local collaboration

`openwork-collab` is the BYOA collaboration runtime. It runs only on macOS. One OpenWork Desktop supervises one local Server process and one local Computer process. The current production Engine adapter is OpenCode.

## Module map

```text
src/
├── protocol/    typed HTTP/SSE payloads shared across process seams
├── server/      business rules, PostgreSQL facts, Redis coordination, HTTP/SSE
├── computer/    desired-state reconcile, AgentRunner, Agent home, Engine adapters
├── process.rs   child-process bootstrap and bounded shutdown
└── bin/
    ├── openwork-collab.rs   Server/Computer role entry point
    └── openwork.rs          short-lived Agent command shim
```

The three external interfaces are intentionally small:

- Desktop sends `DesktopCommand` and receives `DesktopCommandResult`.
- Computer fetches desired Agent state, reports observed runtime state, and receives management invalidations.
- An AgentRunner uses an Agent JWT to read durable work and submit typed `AgentCommand` values.

Server business modules own their SQL directly. There is no universal repository or pass-through manager. Computer has no database or Redis dependency. Server never starts an Engine process.

## Runtime topology

```text
React WebView
  → Tauri commands
    → Desktop HTTP + one Desktop SSE
      → Collaboration Server

Local Computer
  → Computer HTTP + one management SSE
  → one Agent SSE per active AgentRunner
  → per-Agent OpenCode child process

Agent command shim
  → typed Agent HTTP command

Collaboration Server
  → PostgreSQL durable facts
  → Redis expiring coordination and invalidation hints
```

Each Desktop start creates a new RuntimeSession, Desktop secret, Computer secret, and short-lived Agent JWTs. If one of the two child processes exits, Desktop replaces both children. It also rotates the whole RuntimeSession. A normal Desktop exit stops Computer and Engine processes first, then stops Server.

## Engine seam

`EngineAdapter` owns Engine-wide probing and runtime creation. `AgentEngineRuntime` owns one Agent's session-bearing turn execution and shutdown. `EngineRegistry` currently registers only `OpenCodeAdapter`. Tests also register a fake adapter. The fake adapter lets the tests verify the interface without a Provider call.

To add another Engine, add a working adapter and register it. Do not add placeholder adapters, fallback selection, or a capability matrix before a second production Engine exists.

## State ownership

- PostgreSQL stores participants, Agent profile/config, rooms, messages, Climate, Board/Column/Card, Run/delivery/triage, command idempotency, and last Engine observation.
- Redis stores only expiring seen/HELD/rate/cooldown state and Pub/Sub invalidations. A Redis failure cannot erase a durable message.
- `~/.openwork/agents/<agent-id>/` stores the managed persona contract, private `work`, and minimal per-Engine session continuity.
- `~/.openwork/runtime/<runtime-session-id>/` stores the current shim, runtime tokens, and derived Engine configuration. A normal exit removes it.

## Verification

Run the deterministic tests with isolated PostgreSQL databases and the configured local Redis:

```sh
TEST_DATABASE_URL=postgres://openwork:openwork@127.0.0.1:5432/openwork \
TEST_REDIS_URL=redis://127.0.0.1:6379/15 \
cargo test -p openwork-collab
```

`openwork-desktop` owns the full Desktop supervisor test:

```sh
TEST_DATABASE_URL=postgres://openwork:openwork@127.0.0.1:5432/openwork \
TEST_REDIS_URL=redis://127.0.0.1:6379/15 \
cargo test -p openwork-desktop --test collab_supervisor
```

The test runs the real supervisor, Server, Computer, shim, and fake OpenCode. It replaces the process group after Server and Computer crashes. It also verifies a bounded normal shutdown.

The real OpenCode smoke test is opt-in because it makes a real model request:

```sh
OPENWORK_REAL_OPENCODE_SMOKE=1 \
OPENWORK_REAL_OPENCODE_MODEL=deepseek/deepseek-flash \
TEST_DATABASE_URL=postgres://openwork:openwork@127.0.0.1:5432/openwork \
TEST_REDIS_URL=redis://127.0.0.1:6379/15 \
cargo test -p openwork-collab --test runtime_e2e desktop_server_computer_and_real_opencode_smoke
```

The login and Provider configuration come from the local OpenCode installation.
