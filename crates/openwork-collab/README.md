# OpenWork collaboration P0 spikes

This crate is an executable technical spike for `docs/collaboration.md` P0. It contains no
collaboration business logic or persistence. Every binary starts an isolated `opencode serve`,
uses a temporary working directory, prints the exact HTTP/SSE evidence it observes, and removes
the temporary directory when it exits.

## Prerequisites

- `opencode` 1.18.18 is on `PATH` and already has a usable provider login.
- The process may read OpenCode's user auth/config files and write OpenCode's own log directory.
- No `OPENCODE_SERVER_PASSWORD` is required. If it is set, the bins automatically use HTTP basic
  auth with username `opencode` and that password.
- Set `OPENCODE_BIN=/absolute/path/to/opencode` to test a binary other than the one on `PATH`.

## Run

Run one spike at a time so its event chronology stays easy to inspect:

```sh
cargo run -p openwork-collab --bin spike1_drive
cargo run -p openwork-collab --bin spike2_context
cargo run -p openwork-collab --bin spike3_busy
cargo run -p openwork-collab --bin spike4_mcp
cargo run -p openwork-collab --bin spike5_permission
```

What each binary proves:

1. `spike1_drive`: starts the server, probes both route families, creates a v1 session with
   `x-opencode-directory`, sends `prompt_async`, and extracts final text plus tokens from `/event`.
2. `spike2_context`: obtains `id` from `POST /session`, reuses it in two
   `/session/{id}/prompt_async` calls, and checks that round two recalls an exact fact.
3. `spike3_busy`: waits until the first round's `bash` tool is visibly running, sends a second
   prompt, records the complete HTTP response, and classifies the event/message chronology.
4. `spike4_mcp`: starts an official-`rmcp` Streamable HTTP echo server, writes the exact remote MCP
   config to the temporary `opencode.json`, and verifies both the `openwork_echo` tool call and
   configured token.
5. `spike5_permission`: reads two files outside the session directory; it approves the first with
   `once`, rejects the second with a marker message, and verifies what the model receives.

Each success path ends with `SPIKE<n>_RESULT=成立`; spike 3 instead prints `SPIKE3_OUTCOME=...`
because all three documented outcomes are valid findings. A non-zero exit means the stated
acceptance condition was not observed; do not reinterpret it as a pass.

## Lifetime of this code

Everything here is P0 scaffolding with a planned end date. When P1 starts:

- **`src/lib.rs` is promoted.** The parts already proven by the spikes — spawning `opencode serve`
  and parsing its ready line for the random port, the SSE frame splitter, and event
  normalisation — become real modules of the daemon.
- **`src/bin/spike*.rs` are deleted.** They are one-shot probes, not examples and not tests. Their
  findings live in `SPIKE-P0.md`; once those findings are folded into `docs/collaboration.md`,
  the probes have no remaining reader.
- **`SPIKE-P0.md` and `API-RESEARCH.md` are deleted with them.** Conclusions that still matter must
  by then be in `docs/`, which is the only place that describes the target state. Anything not
  worth moving there was not worth keeping.

Keep shared logic in `lib.rs` and one-shot probe flow in `bin/` so this deletion stays a clean cut.
This crate must never depend on another OpenWork crate; `cargo tree -p openwork-collab` is the check.
