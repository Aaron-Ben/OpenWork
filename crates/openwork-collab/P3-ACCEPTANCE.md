# P3 acceptance record

Date: 2026-08-19 (Asia/Shanghai)
Host: macOS, OpenCode 1.18.18, PostgreSQL in `openwork-postgres`

The manual run used PostgreSQL schema `collab_p3_acceptance_20260819` and daemon home
`/private/tmp/openwork-p3-acceptance-20260819`. No normal collaboration tables were used. The
schema and home were removed after recording the results.

## Real daemon, support model, and two Agents

The daemon started with its normal OpenCode supervisor and the existing encrypted DeepSeek
credential. `triage-config` selected provider model `deepseek-v4-flash`; triage went directly to
that provider API, while Alice and Bob used their OpenCode sessions.

An informational human message produced two `support_model` records with
`actionable=false`. One representative record was:

```text
agentId=alice  upToSequence=1  actionable=false  source=support_model
reason="Message is informational and explicitly states no response is needed."
inputTokens=174  outputTokens=40  latencyMs=1376
```

Immediately afterwards both `opencodeSessionId` values were still null. The daemon had triaged the
message but had not woken either Agent.

The next message explicitly addressed Alice. Triage recorded Alice as actionable and Bob as not
actionable, and the durable room stream became:

```text
2  user   @alice Publish exactly P3_REAL_ALICE through openwork_reply. Bob should not publish prose.
3  alice  P3_REAL_ALICE
```

This is a real OpenCode turn and authenticated MCP `reply`, not a direct database insert.

## Asymmetric failure

Only the isolated schema's `collab_settings` row was temporarily pointed at a missing provider.
An Agent-authored message at sequence 4 recorded:

```text
agentId=bob  actionable=false  source=fail_closed
reason="triage provider was not found: missing-provider"
```

Bob's session id remained null, proving the pure Agent case did not wake him. A human-authored
message at sequence 5 then produced `fail_open`, `actionable=true` rows for both candidates. Bob was
woken and published `P3_FAIL_OPEN_BOB` at sequence 6. Alice's main reasoning saw that Bob, not Alice,
was named and stayed quiet; this also confirms that the daemon did not use `responseMode` to select
one candidate.

## HELD, seen, restart, and DM

The real PostgreSQL concurrency test releases Alice and Bob from a barrier with the same seen
sequence. Exactly one reply is published at sequence 2. The stale writer receives HELD with
`peer_sequence=2` and that peer message, then the **same held Agent** retries from sequence 2 and
publishes at sequence 3. Assertions require one row at each sequence and no duplicate body.

The same test module proves that a two-member DM bypasses HELD entirely. Pure coordination tests
cover ten-minute monotonic seen expiry/fail-open and 120-second HELD-token expiry and identity/room
binding. A storage regression test observes sequence 9 in the in-memory coordination hub and then
asserts the Agent inbox is byte-for-byte unchanged.

The manual daemon was stopped and restarted against the same schema and home. Alice and Bob retained
session ids `ses_fe6bebe6cffedhMlAyGiDd8g5k` and `ses_fe6bda446ffe5tuZC4xV0oKffj`;
runtime activity returned to `idle`. After all glance, reply, reaction, restart, and retry activity,
PostgreSQL still reported:

```text
alice|0
bob|0
user|0
```

for `collab_room_members.last_read_seq`. Seen state therefore cleared with the process without
touching the persisted inbox cursor.

## React and runtime status

During a real Alice turn that executed `bash` with `sleep 12`, `agent-list` returned:

```json
{"id":"alice","activity":{"kind":"executing","detail":"bash"}}
```

After the tool completed, Alice published `P3_TOOL_STATUS` and returned to `idle`. Runtime and idle
events came from the Agent home's instance-scoped `GET /event`; the single `GET /global/event`
stream remained dedicated to cross-instance approvals. A loopback SSE contract test asserts the
`x-opencode-directory` header and payload normalization. The Desktop receives only daemon states,
never OpenCode event names or directories.

The first manual wording for “react, do not reply” exposed a real triage prompt bug: the support
model treated “no prose” as “not actionable,” so Bob never got a chance to call `react`. The prompt
now states that speaking, reacting, and requested actions are all actionable. Repeating the scenario
recorded `actionable=true` for Bob and produced exactly this reaction row:

```text
msg_67ec25d60e044d0db638a6586a276cec|bob|👍
```

No Bob-authored prose was added. The provider integration test now guards the corrected reaction
instruction.

## Long-room and Desktop checks

The PostgreSQL paging test bulk-loads 10,000 messages, persists the human read cursor at sequence
5,000, and requires the around-cursor query to return only 50 messages (4,975–5,024) within a
two-second ceiling, with both paging directions present. The query bounds both index ranges before
ranking nearby sequences; it does not sort the entire room by distance. The Desktop store test
asserts that opening a room sends no explicit newest anchor and keeps only the returned window.

Desktop reducer/store tests additionally prove:

- normalized Agent activity updates the matching roster entry without a refetch;
- a HELD event records a room-level “yielded once” notice but never inserts a chat message;
- unread totals exclude muted rooms while their row-level unread count remains visible;
- gaps still cause canonical refresh, while activity and HELD events use their direct paths;
- the Rust event envelope serializes `agentId`, `roomId`, and `peerSequence` in the camel-case shape
  consumed by Desktop.

## Acceptance mapping

| Check | Evidence |
|---|---|
| Unrelated message does not wake | Real `actionable=false` support-model rows; both session ids remained null |
| Failure closes for Agents, opens for humans | Real missing-provider rows and `P3_FAIL_OPEN_BOB` durable reply |
| Two-way reply competition | Barrier-based real PostgreSQL HELD/retry test; same held Agent retries; unique sequences 2 and 3 |
| DM bypasses HELD | Real PostgreSQL two-member test returns `Published` directly |
| Seen does not affect inbox | Coordination + PostgreSQL inbox regression; only human mark-read code writes `last_read_seq` |
| Restart clears seen, keeps durable state | Real daemon restart retained both session ids; all persisted cursors remained zero |
| Teammate already answered → react | Real Bob `👍` row and no Bob prose after Alice's answer |
| Event-driven roster activity | Real `executing { detail: "bash" }` sample, then `idle`; `/event` header/payload test |
| HELD visible without duplicate content | Daemon `reply_held` event + Desktop controller/store tests; camel-case wire-contract test |
| Interrupted/injected turn is not treated as delivered | Wake-state pending-rerun test; neither prompt injection nor seen updates inbox/read cursors |
| 10,000-message room stays bounded | Real PostgreSQL 10k paging test plus Desktop bounded-window store test |
| Open from unread and load both ways | Around-5,000 result has `hasOlder=true` and `hasNewer=true`; Desktop opens with daemon default anchor |
| Muted unread remains local | Existing room-store test keeps row count while excluding it from global badges |

## Gates

The final working tree passed:

```text
npm --prefix desktop run typecheck                         PASS
npm --prefix desktop test                                  PASS (70 files, 407 tests)
cargo fmt --all -- --check                                 PASS
cargo clippy --workspace --all-targets -- -D warnings      PASS
TEST_DATABASE_URL=postgres://openwork:openwork@localhost:5432/openwork cargo test --workspace
                                                            PASS
cargo tree -p openwork-collab                              PASS
```

The full workspace run executed all five collaboration PostgreSQL tests, including the HELD race
and 10,000-message room. Two existing live-model tests remained ignored because they require
separate external-key conditions. The first full workspace attempt had one unrelated, unchanged
`openwork-core` PostgreSQL test report `turn not found`; the exact test passed immediately in
isolation (1.99s), and the required full workspace command then passed end to end on rerun. This
record does not hide the initial nondeterministic failure.

The dependency tree contains only `openwork-models` and `openwork-credentials` among OpenWork
crates. `openwork-core`, `openwork-agent`, `openwork-chat-state`, and `openwork-tools` do not appear
under `openwork-collab`.
