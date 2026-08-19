# P6 acceptance record

Date: 2026-08-19 (Asia/Shanghai)  
Host: macOS, OpenCode 1.18.18, PostgreSQL in `openwork-postgres`

The controlled live run used PostgreSQL schema `openwork_p6_accept_20260819` and daemon home
`/private/tmp/openwork-p6-accept-home`. The daemon and its OpenCode child were stopped normally;
the schema and home were then removed. No normal collaboration tables, rooms, or Agent homes were
changed.

## Schema and best-effort event path

Migration `202608190004_collab_p6.sql` creates only `collab_events`, after every prior migration on
an empty schema. It matches the data-model DDL: all timestamps are `TIMESTAMP WITHOUT TIME ZONE`
with the Asia/Shanghai default, the JSON payload must be an object, and `kind` deliberately has no
CHECK constraint. The PostgreSQL timeline test inserts a future unknown kind and proves it remains
accepted and visible.

Prompt, normalized OpenCode, MCP speech, and triage call sites submit observations through a
bounded `try_send` queue. A worker performs every event insert and logs insert failures without
returning them to dispatch, reply, claim, or triage. Thus observation storage cannot await on or
fail the main collaboration path. Exact repeated OpenCode frames are suppressed per active run;
usage snapshots are aggregated by assistant message id before updating `collab_runs`.

The durable event vocabulary exercised in P6 is:

```text
triage.decision
prompt.started / prompt.injected
prompt.completed / prompt.failed / prompt.interrupted
tool.execution / command.execution
speech.published
usage.reported
```

The normalizer keeps tool state, bounded command/title/error details, message ids, and token fields.
Zero-token assistant placeholders and unrelated OpenCode events do not enter the observation table.

## Controlled complete wake timeline

The isolated daemon used the configured cheap provider for triage and Agent `alice` on
`opencode/hy3-free`. The human message was:

```text
@alice Run the harmless command printf P6_COMMAND_OK in your home, then publish exactly
P6_LOG_FLOW_OK through openwork_reply.
```

The durable room stream ended with Alice's exact `P6_LOG_FLOW_OK` reply at sequence 2. The merged
log IPC returned the following ordered facts for one run
`run_944be16f757e4adb9ed7702de6bb3cf5`:

```text
22:46:48  triage.decision   actionable=true, source=support_model
          inputTokens=218, outputTokens=60, latencyMs=1704
22:46:50  prompt.started    trigger=message
          command.execution tool=bash, command="printf P6_COMMAND_OK",
                            status=pending → running → completed
          tool.execution    tool=openwork_reply,
                            status=pending → running → completed
          speech.published  sequence=2
          usage.reported    assistant message usage
22:47:04  prompt.completed  status=completed
```

The final `collab_runs` row was `status=completed` with input `21857`, cached input `42368`, and
output `93` tokens. Every timestamp returned by the log IPC ended in `+08:00`.

This live run also exposed that OpenCode may emit byte-for-byte identical usage/tool state frames.
The worker now fingerprints normalized entries per active run. The real PostgreSQL worker test sends
an identical usage snapshot twice, requires only one durable copy, and still requires the run total
to use the latest snapshot for each distinct assistant message.

## Flat log drawer

`logs [room-id]` and the Desktop Tauri command read one bounded timeline assembled from
`collab_runs`, `collab_triages`, and `collab_events`. Each item retains its source, raw kind,
run/Agent/room ids, Beijing timestamp, summary, and expandable JSON payload. Results can be global
or scoped to the active room.

The Rail's existing **Logs** destination now renders that list directly as an `<ol>`. It has no
Span hierarchy, completeness derivation, trace token semantics, OpenCode event-name interpretation,
or collaboration decisions. A committed observation publishes `logs_changed`; the root event bridge
refreshes an open log destination without polling or a manual reload.

Rust tests require the merged result to contain all three sources in time order and preserve an
unknown future event kind. Vitest covers the store, room filter, event-controller invalidation,
flat rendering, localized summaries, and the absence of a Span marker.

## GC retention boundary

The independent worker sweeps immediately on daemon startup and then once per day. Its defaults are
30 days for events, 30 days for triages, 500 rows per table per batch, and a 2000 ms PostgreSQL
statement timeout. Each table batch has its own short transaction and transaction-local
`statement_timeout`; successive batches yield to the runtime.

The real PostgreSQL test creates two observations on the expired side of the 30-day boundary and
one on the retained side for both tables. With a batch size of one, the first call must delete
exactly one row from each table; subsequent batches delete the other expired rows and retain both
fresh rows. In the same schema it creates one room message and one completed run, then proves both
counts remain exactly one after GC. No GC SQL references `collab_messages` or `collab_runs`.

## Three-language completeness

Simplified Chinese, Traditional Chinese, and English now contain the same complete collaboration
key tree, including every log control, source badge, decision summary, token summary, loading state,
and empty state. The existing locale-key traversal was strengthened with explicit log-label checks:
it compares the complete leaf-key set of all three resources and also rejects any log label that
falls back to its key name.

## Throughout-the-project checks

- `cargo tree -p openwork-collab` contains only `openwork-models` and
  `openwork-credentials` among OpenWork crates; `openwork-core` is absent.
- `crates/openwork-core/migrations/` has no `collab_*` migration or edit from P6.
- Collab OpenCode calls remain on the v1 non-`/experimental/` path family. The only `/api/session`
  occurrence is a source comment explaining why that path family is not used.
- Desktop displays daemon-provided log records and normalized invalidations; it does not infer
  HELD, deduplication, ownership, triage, or event meanings.
- The P6 migration uses the required Asia/Shanghai default and all newly returned times pass the
  `+08:00` PostgreSQL assertion.
- No file under `docs/` was changed.

## Acceptance mapping

| Check | Evidence |
|---|---|
| Complete wake can be reconstructed | Controlled real daemon timeline from support-model triage through exact MCP reply, usage, and completion |
| Observation cannot stall main work | Bounded non-awaiting producer; worker-only inserts with logged failures |
| Continuous growth is bounded | Independent daily worker, bounded timeout transactions, real PostgreSQL expired/fresh boundary test |
| Messages survive GC | The same PostgreSQL test requires `collab_messages=1` and `collab_runs=1` after cleanup |
| Three locale sets are complete | Full recursive key-set equality plus explicit no-fallback log-label test |
| No second Trace UI | Flat three-source storage query and flat ordered-list rendering test |

## Gates

The final working tree passed:

```text
npm --prefix desktop run typecheck                         PASS
npm --prefix desktop test                                  PASS (75 files, 420 tests)
cargo fmt --all -- --check                                 PASS
cargo clippy --workspace --all-targets -- -D warnings      PASS
TEST_DATABASE_URL=postgres://openwork:openwork@localhost:5432/openwork cargo test --workspace
                                                            PASS
cargo tree -p openwork-collab                              PASS
```

The required workspace command ran every collaboration PostgreSQL test, including the real GC
boundary tests. Two unrelated live-provider tests remained ignored because their explicit
credential/network preconditions were absent. The known intermittent core `turn not found` failure
did not occur.

## Process findings not yet absorbed into `docs/`

The P2–P6 acceptance files remain in place for a human documentation pass. Comparing their material
findings with the current `docs/` leaves these implementation/operational conclusions unabsorbed:

- **P3:** an explicitly requested reaction or other non-prose action must still be classified
  `actionable`; otherwise the main Agent never gets a chance to call `react`.
- **P4:** rmcp rejects an enum as the root tool input schema with
  `Schema is missing 'type' field`; the tool request must be a root JSON object containing the
  action enum. The current 15-second claim check and 60-second dead-session grace period are also
  documented only in the README/acceptance record, not the design docs.
- **P5:** scanner fingerprints must exclude the scanner's own proactive marker/reply, and an
  actionable stalled-room claim must remain held through main dispatch. The concrete idle,
  scanner, rate, and cooldown intervals are recorded in README/acceptance rather than target docs.
- **P6:** OpenCode 1.18.18 can repeat identical usage/tool state frames, so durable observations
  need entry fingerprinting. The four GC environment names and 30-day/500-row/2000-ms defaults are
  operational choices recorded in README, not design decisions in `docs/`.

P2 has no remaining product conclusion to carry over: its native macOS capture/drag limitation is
already recorded as `R-F7` in `docs/collaboration-desktop.md`. Test-run incidents such as the
pre-existing core flake and shared-schema test isolation are process evidence, not collaboration
target-state conclusions.
