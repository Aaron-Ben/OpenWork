# P5 acceptance record

Date: 2026-08-19 (Asia/Shanghai)  
Host: macOS, OpenCode 1.18.18, PostgreSQL in `openwork-postgres`

The live run used the isolated PostgreSQL schema `openwork_p5_accept_20260819` and daemon home
`/private/tmp/openwork-p5-accept-home`. The daemon was stopped immediately after one agenda turn;
the schema and home were then deleted. No normal collaboration room, Agent home, or board was
changed, and no unattended daemon from this run remains.

## Idle and agenda gate

The worker checks once per minute and only considers enabled Agents that have been quiet for at
least 90 seconds. One in-memory rotation cursor selects the next available Agent. Storage builds
focused candidates from cards in columns whose stored `is_done` value is false and from rooms
stalled between 5 minutes and 6 hours; no column-title classification exists.

When no candidate exists, `sweep_agenda` writes an `empty_inbox`, `actionable=false` triage record
without loading the provider configuration. The real PostgreSQL test
`empty_agenda_records_the_reason_without_starting_a_main_run` also requires
`SELECT count(*) FROM collab_runs` to remain zero. This is the no-main-token path.

When a candidate exists, the configured triage provider is called directly before OpenCode. A
provider/configuration failure resolves to `fail_open`, so a broken small model cannot silence real
candidate work. The provider integration test verifies that message triage, agenda, and the DM
detector all use the configured OpenWork credential and cheap model API.

## Controlled live agenda turn

The setup created Alice, room `general`, board `work`, explicit `Todo(isDone=false)` and
`Done(isDone=true)` columns, and one assigned card. The daemon was stopped inside the normal
2.5-second message debounce and restarted, so no message-driven wake survived; the subsequent run
could only originate from the periodic agenda path.

The cheap provider recorded:

```text
source=support_model  actionable=true  provider=prov-92624551db0247cc8938fe67518a093e
model=deepseek-chat   inputTokens=438   outputTokens=84   latencyMs=1993
reason="The agenda contains an assigned card for alice that is unfinished and explicitly requires action (claim, finish, reply)."
```

Only one main run was created:

```text
trigger=agenda  status=completed  provider=opencode  model=hy3-free  ended_at=present
SELECT count(*) FROM collab_runs = 1
```

The durable room sequence was:

```text
1 user  system  card_created, assignedTo=alice
2 alice system  proactive_wake, trigger=agenda, with the gate reason
3 alice system  card_claimed
4 alice system  card_moved, columnId=done
5 alice normal  P5_AGENDA_DONE
```

The final board row was in the explicit done column and retained `claimedBy=alice`. This proves the
small-model gate ran first, supplied the focused brief, one OpenCode main turn used the existing
session/MCP path, and the Agent's proactive result was durable. The structured sequence-2 marker is
what Desktop renders as “Started by agenda”; it does not parse message prose.

## Stalled-room safeguards

`NudgeTracker` is daemon memory only. Its keys are room ids, never message ids. It combines:

- one in-flight claim shared by all Agents, with a five-minute abandoned-claim safety release;
- a 45-minute room cooldown after an unnecessary verdict or a successfully submitted push;
- a three-decline cap, reset on every newly committed room notice;
- clearing prior declines after a necessary nudge.

Pure tests advance synthetic time through all four cases. The real PostgreSQL concurrency test
creates one stalled room visible to Alice and Bob, releases two Tokio tasks from the same barrier,
and requires exactly one `Claimed` plus one `ClaimedByPeer`. If the room sequence changes while the
cheap gate is running, the stale claim is cancelled and no wake is emitted. Database/recording
errors also release the in-memory claim immediately. A separate PostgreSQL regression keeps an
actionable stall claimed between cheap-model approval and main dispatch, so a failed dispatch does
not consume the 45-minute cooldown.

## Agent DM loop detection and rate gates

Agent-to-Agent DMs participate without paying a triage call for messages 1–7. Sequence 8 (and every
multiple of 8) invokes the cheap progress detector. The PostgreSQL plus loopback-provider test
`agent_dm_defaults_to_engage_then_stops_at_the_eighth_no_progress_message` constructs eight
repetitive messages and proves:

```text
sequence 7 -> actionable=true, source=dm_agent_engage, provider calls=0
sequence 8 -> actionable=false, source=loop_cap, provider calls=1, inputTokens=31
```

The existing P3 asymmetric fallback remains unchanged: pure Agent model failures are fail-closed.
Before any autonomous main call, the scheduler also enforces one active turn token and a per-Agent
60-second rate gate. Agenda and scanner wakes arriving in the same burst are therefore collapsed;
short circuits persist as `loop_cap` or `rate_limited`. No quota gate was added.

## Scanner

Migration `202608190003_collab_p5.sql` adds only
`scanner_enabled BOOLEAN NOT NULL DEFAULT FALSE`. Desktop and CLI require an explicit opt-in.
Fingerprints, their six-hour seen set, and the baseline are daemon memory only.

The PostgreSQL scanner test proves disabled Agents are absent, the first eligible 24-hour snapshot
is a no-cost baseline, an unchanged snapshot does not wake, and a peer message changes it once.
Fingerprints use canonical room/peer-sequence pairs: the scanner's own proactive marker and reply
do not change its fingerprint, preventing a one-wake-per-minute self-loop. The snapshot requires at
least eight recent room messages and includes bounded recent context for the selected main turn.

## Desktop visibility

The daemon inserts a structured `kind=system` payload before an autonomous dispatch:

```json
{
  "type": "proactive_wake",
  "trigger": "agenda",
  "agentId": "alice",
  "reason": "one unfinished assigned card"
}
```

`rooms_changed` refreshes the existing bounded message window. The Desktop helper accepts only
daemon-tagged `agenda` / `scanner` payloads and renders a distinct badge plus reason. Vitest rejects
a normal message and an unknown trigger, so the frontend makes no collaboration semantic decision.
All three locales include the marker and scanner opt-in labels.

## Acceptance mapping

| Check | Evidence |
|---|---|
| Unfinished card wakes an idle Agent | Controlled real daemon run: support-model gate, one `agenda` run, claim, move, exact MCP reply |
| No actionable item spends no main turn | Empty-agenda PostgreSQL test: `empty_inbox` row and zero `collab_runs` |
| One stalled-room nudge per cooldown | Room-only synthetic-time test plus one-winner PostgreSQL barrier test |
| Three declines stop, new message resets | Pure clocked test; scheduler resets on every committed `MessageNotice` |
| Agent DM stops at message 8 | Real PostgreSQL + loopback cheap-provider test records `loop_cap` and no wake |
| Scanner ignores unchanged/self-produced state | Explicit-opt-in PostgreSQL baseline/change/self-message test plus canonical fingerprint tests |
| Agenda is visibly proactive | Live structured marker at sequence 2 plus Desktop payload/rendering test |

## Gates

The final working tree passed:

```text
npm --prefix desktop run typecheck                         PASS
npm --prefix desktop test                                  PASS (73 files, 414 tests)
cargo fmt --all -- --check                                 PASS
cargo clippy --workspace --all-targets -- -D warnings      PASS
TEST_DATABASE_URL=postgres://openwork:openwork@localhost:5432/openwork cargo test --workspace
                                                            PASS
cargo tree -p openwork-collab                              PASS
```

The first full workspace run found a deterministic test-isolation defect rather than the known
`turn not found` flake: a core migration test enumerated every table in the shared `public` schema
and rejected the already legitimate `collab_*` tables. Its query now excludes only the collab
namespace while continuing to require the exact core table set. The target test and the exact full
workspace command then passed. The known intermittent `turn not found` failure did not occur.

Two unrelated live-provider tests remained ignored because their explicit API-key/network
preconditions were absent. All required PostgreSQL collaboration tests ran. Among OpenWork crates,
`cargo tree -p openwork-collab` contains only `openwork-models` and `openwork-credentials`; it does
not contain `openwork-core`, `openwork-agent`, `openwork-chat-state`, or `openwork-tools`.
