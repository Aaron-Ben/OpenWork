# OpenWork collaboration daemon

`openwork-collab` is the persistent backend for collaboration mode. It carries room messages from
the Desktop or CLI to an `@mentioned` OpenCode Agent and publishes the Agent's authenticated MCP
reply back into the room. The daemon is the only writer of `collab_*` data; Desktop and every CLI
command below talk to its Unix socket.

## Prerequisites

- PostgreSQL is reachable through `DATABASE_URL` (default:
  `postgres://openwork:openwork@localhost:5432/openwork`).
- `opencode` is on `PATH` and already logged in. The daemon does **not** compare version numbers;
  it runs a startup self-check against the behaviour it actually depends on (the v1 `/session`
  family and its array shape, `/agent`, and a connectable `/global/event`). Any probe that fails
  rejects startup before `daemon.ready=true` and names which one. The version is reported in
  diagnostics only. See `docs/collaboration.md` §4.2.
- `OPENWORK_API_KEY_ENCRYPTION_KEY` is set when using `credential-check`. The daemon loads the
  repository `.env` in the same way as the Desktop host.
- `OPENWORK_COLLAB_HOME` optionally changes the state root. The default is
  `~/.openwork/collab`.

## Start and stop

Run the daemon in a terminal:

```sh
cargo run -p openwork-collab --bin openwork-collab -- daemon
```

The ready sequence prints the socket, repaired run count, MCP address, verified OpenCode version,
child PID/generation, and finally `daemon.ready=true`. A second invocation refuses to start with a
message naming the occupied socket.

Inspect or stop it from another terminal:

```sh
cargo run -p openwork-collab --bin openwork-collab -- status
cargo run -p openwork-collab --bin openwork-collab -- shutdown
```

`shutdown` is graceful: it cancels scheduling, stops `opencode serve` and MCP, closes the socket,
and waits for the workers. Closing Desktop is not a shutdown operation.

## Connect from Desktop

Run the Tauri app normally:

```sh
npm --prefix desktop run tauri dev
```

`src-tauri` first pings `OPENWORK_COLLAB_HOME/daemon.sock`. If no live daemon owns the socket, it
starts the same Desktop executable in daemon-only mode and waits for the socket to answer. The
daemon is a separate process: closing Desktop deliberately leaves it running. Use the CLI
`shutdown` command when you actually want to stop it.

All WebView reads and writes are Tauri commands; the WebView never opens the Unix socket or reads
`collab_*` tables. A long-lived host task subscribes to the daemon's versioned event stream and
emits `openwork://collab-event` (or an ordered batch) to the root-level collaboration bridge.
That bridge remains mounted in both modes, which is why the workbench sidebar can show room unread
and pending-approval badges while the collaboration Shell is not visible.

Use the workbench sidebar's **Collaboration** button to enter the second Shell. From there:

1. create a room and Agents in the **Teammates** destination;
2. add enabled Agents from the room roster;
3. send `@agent_id ...` in the room input;
4. handle permanent OpenCode asks in the red approval cards with **Allow once**, **Always allow**,
   **Reject** plus a reason, or **Abort run**.

Room history opens through a bounded `sequence` page around the human user's persisted
`last_read_seq`; **Load older** and **Load newer** extend only the in-memory window. The Mark read
button is the only Desktop action that advances that human cursor. Agent inbox delivery never uses
it.

## Create two Agents and verify the backend path

Arguments containing spaces must be shell-quoted.

```sh
cargo run -p openwork-collab --bin openwork-collab -- \
  agent-create alice Alice opencode hy3-free \
  'When explicitly mentioned, answer through openwork_reply.'

cargo run -p openwork-collab --bin openwork-collab -- \
  agent-create bob Bob opencode hy3-free \
  'When explicitly mentioned, answer through openwork_reply.'

cargo run -p openwork-collab --bin openwork-collab -- room-create general General
cargo run -p openwork-collab --bin openwork-collab -- room-add general alice
cargo run -p openwork-collab --bin openwork-collab -- room-add general bob

cargo run -p openwork-collab --bin openwork-collab -- \
  send general user '@alice Reply with exactly P1_ALICE_REPLY using openwork_reply.'
```

Wait for the 2.5-second debounce and the model turn, then inspect the durable stream:

```sh
cargo run -p openwork-collab --bin openwork-collab -- messages general
cargo run -p openwork-collab --bin openwork-collab -- agent-list
```

The expected evidence is an Alice-authored `P1_ALICE_REPLY` after the user message, with consecutive
room sequences. Bob's `opencodeSessionId` remains null because `@` wakes only the named Agent.

To update an Agent, use the same argument shape with `agent-update`. The daemon immediately repairs
that home, and every dispatch reloads the current database definition, rewrites managed files, and
sends the current prompt/model to `prompt_async`:

```sh
cargo run -p openwork-collab --bin openwork-collab -- \
  agent-update alice Alice opencode hy3-free \
  'Prefix every published reply with V2:'
```

## Operational checks

- `permissions` returns the daemon's cross-instance pending set built from one
  `GET /global/event` subscription. It never uses an unscoped `GET /permission`.
- `credential-check <provider-id>` proves that collab can read and decrypt the same
  `provider_credentials` row as core. It returns metadata plus `decrypted: true`, never the key.
- `status` returns the OpenCode PID and generation. To verify restart recovery, record the Agent's
  `opencodeSessionId`, terminate that exact child PID, wait for generation to increment, then run
  `agent-list` again. The session id must be unchanged. An active run becomes `interrupted`; the
  scheduler creates a `rerun` because unread delivery was never cleared by injection.
- On every daemon startup, `opencode.json` and `AGENTS.md` are overwritten from the database and the
  new process token/MCP address. `memory/MEMORY.md` is created only when missing and is never
  overwritten.

## Tests

Pure domain and process tests:

```sh
cargo test -p openwork-collab
```

The PostgreSQL test uses a random empty schema, migrates twice, exercises concurrent sequence
allocation and exact deduplication, and drops only that test-owned schema:

```sh
TEST_DATABASE_URL=postgres://openwork:openwork@localhost:5432/openwork \
  cargo test -p openwork-collab --test postgres
```

Repository acceptance checks:

```sh
npm --prefix desktop run typecheck
npm --prefix desktop test
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
TEST_DATABASE_URL=postgres://openwork:openwork@localhost:5432/openwork cargo test --workspace
cargo tree -p openwork-collab
```

`TEST_DATABASE_URL` is not optional here. Without it the PostgreSQL test **skips itself silently**,
so the run still reports green while concurrent sequence allocation and exact deduplication — the
two invariants a single writer is supposed to guarantee — were never exercised.

The dependency tree may contain only the two approved OpenWork dependencies:
`openwork-models` and `openwork-credentials`; it must not contain `openwork-core`,
`openwork-agent`, `openwork-chat-state`, or `openwork-tools`.

The dated manual and automated P2 evidence is in [`P2-ACCEPTANCE.md`](P2-ACCEPTANCE.md).

## Where the P0/P1 findings live

The P0 probes (`src/bin/spike*.rs`) and their reports (`SPIKE-P0.md`, `API-RESEARCH.md`) are gone.
Everything they proved is now either running code or design documentation:

- process startup, ready-line parsing, the v1 client, SSE framing and global-event normalization →
  `src/opencode.rs`; the official `rmcp` server pattern → `src/mcp.rs`;
- the API facts and their consequences → `docs/collaboration.md` §4 (endpoints, self-check),
  §6 (why a single `/global/event` carries every agent's pending approvals), and §8.1 (what
  happens when a busy session is prompted again).

A finding worth keeping belongs in `docs/`, which is the only place describing the target state.
Anything that was not worth moving there was not worth keeping.
