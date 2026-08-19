# OpenWork collaboration daemon

`openwork-collab` is the persistent backend for collaboration mode. It triages each debounced room
update, runs a cheap agenda gate before any autonomous main-Agent turn, optionally scans cross-room
changes, and publishes authenticated MCP actions back into rooms. The daemon is the only writer of
`collab_*` data; Desktop and every CLI command below talk to its Unix socket.

## Prerequisites

- PostgreSQL is reachable through `DATABASE_URL` (default:
  `postgres://openwork:openwork@localhost:5432/openwork`).
- `opencode` is on `PATH` and already logged in. The daemon does **not** compare version numbers;
  it runs a startup self-check against the behaviour it actually depends on (the v1 `/session`
  family and its array shape, `/agent`, the session-id map from `/session/status`, and a connectable
  `/global/event`). Any probe that fails
  rejects startup before `daemon.ready=true` and names which one. The version is reported in
  diagnostics only. See `docs/collaboration.md` §4.2.
- `OPENWORK_API_KEY_ENCRYPTION_KEY` is set when using `credential-check` or triage. Triage decrypts
  the selected OpenWork provider credential and calls its API directly; it never consumes an
  OpenCode Agent session or subscription quota. The daemon loads the repository `.env` in the same
  way as the Desktop host.
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
3. send a message in the room input; `@agent_id` remains a useful signal to the triage model, but
   every eligible Agent makes an independent triage decision;
4. handle permanent OpenCode asks in the red approval cards with **Allow once**, **Always allow**,
   **Reject** plus a reason, or **Abort run**.

Room history opens through a bounded `sequence` page around the human user's persisted
`last_read_seq`; **Load older** and **Load newer** extend only the in-memory window. The Mark read
button is the only Desktop action that advances that human cursor. Agent inbox delivery never uses
it.

## Configure triage

Triage uses one enabled provider credential and one model already configured in OpenWork. Select
them once through the daemon socket:

```sh
cargo run -p openwork-collab --bin openwork-collab -- \
  triage-config <provider-id> <model-id>
```

The provider credential and an enabled `models` row bound to it must exist; `<model-id>` is that
row's provider-facing `model_name`, not its OpenWork database row id. Configuration fails readably
if either is missing or the credential is disabled. The provider API must accept the selected
model. Restart is not required.

Inspect recent decisions globally or for one room:

```sh
cargo run -p openwork-collab --bin openwork-collab -- triage-list
cargo run -p openwork-collab --bin openwork-collab -- triage-list general
```

Each record shows the candidate Agent, covered sequence, `actionable`, model response mode,
reason/prompt note, source, token usage, and latency. `support_model` is a successful model call;
`fail_open` means a triage failure still woke an Agent because a human was waiting;
`fail_closed` means a pure Agent-to-Agent update did not wake another Agent.

## Create two Agents and verify the coordinated path

Arguments containing spaces must be shell-quoted.

```sh
cargo run -p openwork-collab --bin openwork-collab -- \
  agent-create alice Alice opencode hy3-free \
  'Answer only when you can materially help. Publish through openwork_reply.' false

cargo run -p openwork-collab --bin openwork-collab -- \
  agent-create bob Bob opencode hy3-free \
  'Avoid repeating a published answer. Use openwork_react when agreement is enough.' false

cargo run -p openwork-collab --bin openwork-collab -- room-create general General
cargo run -p openwork-collab --bin openwork-collab -- room-add general alice
cargo run -p openwork-collab --bin openwork-collab -- room-add general bob

cargo run -p openwork-collab --bin openwork-collab -- \
  send general user '@alice Reply with exactly P3_ALICE_REPLY using openwork_reply.'
```

Wait for the 2.5-second debounce and the model turn, then inspect the durable stream:

```sh
cargo run -p openwork-collab --bin openwork-collab -- messages general
cargo run -p openwork-collab --bin openwork-collab -- agent-list
cargo run -p openwork-collab --bin openwork-collab -- triage-list general
```

The expected evidence is an Alice-authored `P3_ALICE_REPLY` after the user message, consecutive room
sequences, and one triage row per eligible Agent. The support model—not the daemon—decides whether
Bob is actionable. If Bob sees Alice's published answer before writing, the five standing rules tell
it to react or stay quiet; a stale write in a room with more than two members is rejected as `HELD`.

To update an Agent, use the same argument shape with `agent-update`. The daemon immediately repairs
that home, and every dispatch reloads the current database definition, rewrites managed files, and
sends the current prompt/model to `prompt_async`:

```sh
cargo run -p openwork-collab --bin openwork-collab -- \
  agent-update alice Alice opencode hy3-free \
  'Prefix every published reply with V2:' false
```

## Create a board and inspect claims

Boards belong to rooms; columns belong to boards. Create both explicitly so completion remains the
column's `isDone` field rather than a guess based on its title:

```sh
cargo run -p openwork-collab --bin openwork-collab -- \
  board-create work general 'Shared work'
cargo run -p openwork-collab --bin openwork-collab -- \
  board-column-create todo work Todo 0 false
cargo run -p openwork-collab --bin openwork-collab -- \
  board-column-create doing work Doing 1 false
cargo run -p openwork-collab --bin openwork-collab -- \
  board-column-create done work Done 2 true
```

Open the **Boards** Rail destination to create a card, choose a column, and assign an enabled room
Agent. The same operation is available from the CLI for setup and diagnosis:

```sh
cargo run -p openwork-collab --bin openwork-collab -- \
  card-create work todo 'Verify the P4 migration' alice
cargo run -p openwork-collab --bin openwork-collab -- board-list general
```

`board-list` shows `assigneeId`, `claimedBy`, and `claimedAt`. Agents use the authenticated
`openwork_card` tool with `list`, `create`, `claim`, or `move`; a losing claim returns
`status: "already_claimed"` and the winner's id. Create, claim, move, runtime release, and manual
release each write a structured room system message and enter the normal 2.5-second wake path.
Desktop refreshes the affected room board from `boards_changed`, so claims appear without a manual
reload.

Claims have no TTL. Daemon startup releases all claims without adding room messages. Every 15
seconds the daemon checks claims older than a 60-second grace period against the owning Agent's
instance-scoped `GET /session/status`; a stopped or missing session releases the card and writes a
room system message. Use the card's unlock action, or the following command, for an explicit user
release:

```sh
cargo run -p openwork-collab --bin openwork-collab -- \
  card-release <card-id> <claimant-id>
```

## Autonomous agenda, scanner, and DM loop checks

Autonomy starts only after an Agent has been quiet for 90 seconds. Once per minute, `idle` rotates
across available Agents. Before any agenda wake spends an OpenCode turn, the configured triage
provider evaluates one focused room candidate: incomplete cards use the column's explicit
`isDone=false`, while a stalled room must be between 5 minutes and 6 hours old. No candidate writes
an `empty_inbox` triage record and creates no `collab_runs` row. A gate failure is `fail_open` when a
real candidate exists, so a broken cheap model cannot silently stop all autonomous work.

Inspect why a wake did or did not happen with the existing command:

```sh
cargo run -p openwork-collab --bin openwork-collab -- triage-list general
```

`rate_limited` means the room cooldown or per-Agent autonomous rate gate stopped the wake;
`loop_cap` means a turn token, the three-decline cap, or the eighth-message Agent-DM check stopped
it. Stalled-room cooldown is keyed only by room and lasts 45 minutes. Three unnecessary-nudge
decisions stop further nudges until any new room message resets that count.

Scanner is intentionally off for every existing and newly created Agent. Enable it explicitly in
Desktop's teammate editor, or pass the final boolean to `agent-create` / `agent-update`:

```sh
cargo run -p openwork-collab --bin openwork-collab -- \
  agent-update alice Alice opencode hy3-free \
  'Answer only when you can materially help. Publish through openwork_reply.' true
```

The first eligible 24-hour snapshot is a no-cost baseline. A scanner wake requires at least eight
recent messages and a changed cross-room peer fingerprint; the scanner's own marker/reply cannot
re-arm it, and an unchanged snapshot never wakes the main Agent again. Scanner and agenda share the
per-Agent autonomous rate gate. There is deliberately no quota gate: usage is recorded but not
used to block model calls.

Create or reuse an Agent-to-Agent DM by member set, then inspect every-eighth-message decisions:

```sh
cargo run -p openwork-collab --bin openwork-collab -- dm-create alice bob
cargo run -p openwork-collab --bin openwork-collab -- triage-list
```

Agent DMs participate by default. Messages 8, 16, 24, and so on invoke the cheap progress detector;
a no-progress verdict records `loop_cap` and does not wake the other main Agent.

## Collaboration logs and retention

Open the **Logs** Rail destination for one flat, reverse-chronological timeline assembled from
`collab_runs`, `collab_triages`, and `collab_events`. It is intentionally not a Span tree or a
second Trace UI. Use **All rooms** for daemon-wide diagnosis or **Current room** to follow one
wake through triage, prompt start/end, tool and command states, published speech, and usage.
Committed observations update an open drawer without a manual refresh.

The same bounded feed is available over the daemon socket:

```sh
cargo run -p openwork-collab --bin openwork-collab -- logs
cargo run -p openwork-collab --bin openwork-collab -- logs general
```

Observation persistence is best-effort and runs behind a bounded queue: a slow or failed event
insert is logged and never delays a prompt, reply, or card mutation. `collab_events.kind` remains
open-ended so new normalized OpenCode events do not require a migration.

An independent GC worker runs immediately at daemon startup and then once per day. These variables
configure it; every value must be a positive integer:

| Variable | Default | Meaning |
|---|---:|---|
| `OPENWORK_COLLAB_EVENT_RETENTION_DAYS` | `30` | Days to retain `collab_events` |
| `OPENWORK_COLLAB_TRIAGE_RETENTION_DAYS` | `30` | Days to retain `collab_triages` |
| `OPENWORK_COLLAB_GC_BATCH_SIZE` | `500` | Maximum rows deleted from each table per transaction |
| `OPENWORK_COLLAB_GC_STATEMENT_TIMEOUT_MS` | `2000` | PostgreSQL timeout applied to each delete transaction |

GC deletes only expired `collab_events` and `collab_triages`, in small transactions with a
transaction-local `statement_timeout`. It never deletes `collab_messages` (room history) or
`collab_runs` (which has a separate retention policy). A nonzero sweep prints deleted row counts;
an error is logged and retried on the next daily sweep without stopping the daemon.

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
- `openwork_glance` advances only the daemon's ten-minute in-memory seen cursor. It never updates
  `collab_room_members.last_read_seq`; `openwork_inbox` therefore remains authoritative across
  prompts and restarts. Missing or expired seen state fails open.
- `openwork_reply` applies the HELD freshness gate only to rooms with more than two members. A HELD
  result includes new peer messages plus a 120-second confirmation token; after rereading, the same
  Agent recomputes and retries. `openwork_react` records agreement without publishing duplicate
  prose. Seen cursors and HELD tokens intentionally disappear on daemon restart.
- One `/global/event` stream remains dedicated to cross-Agent approvals. Runtime state and idle
  boundaries come from instance-scoped `/event` streams carrying each Agent home's
  `x-opencode-directory`; those streams are restored after an OpenCode restart.
- `agent-list` reports daemon-normalized `idle`, `busy`, `replying`, `compacting`,
  `executing { detail }`, or `unresponsive` activity. Desktop consumes only these collaboration
  events and never OpenCode event names.

## Tests

Pure domain and process tests:

```sh
cargo test -p openwork-collab
```

The PostgreSQL tests use random empty schemas, run every collab migration in order, and exercise
concurrent sequence allocation, HELD/retry, exact deduplication, reactions, triage persistence,
atomic card claim competition, claim release, agenda card selection, scanner baselining, and the
single in-memory stalled-room pusher over a real room. P6 also covers the three-table log timeline,
duplicate OpenCode observation frames, and both sides of the GC retention boundary while asserting
that messages and runs survive. They drop only their test-owned schemas:

```sh
TEST_DATABASE_URL=postgres://openwork:openwork@localhost:5432/openwork \
  cargo test -p openwork-collab --tests
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

The dated P6 evidence is in [`P6-ACCEPTANCE.md`](P6-ACCEPTANCE.md). Earlier evidence remains in
[`P5-ACCEPTANCE.md`](P5-ACCEPTANCE.md),
[`P4-ACCEPTANCE.md`](P4-ACCEPTANCE.md),
[`P3-ACCEPTANCE.md`](P3-ACCEPTANCE.md) and [`P2-ACCEPTANCE.md`](P2-ACCEPTANCE.md).

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
