# P4 acceptance record

Date: 2026-08-19 (Asia/Shanghai)  
Host: macOS, OpenCode 1.18.18, PostgreSQL in `openwork-postgres`

The manual run used the isolated PostgreSQL schema `openwork_p4_accept_20260819_2035` and daemon
home `/private/tmp/openwork-p4-accept-home`. Both were removed after the evidence below was
recorded; no normal collaboration tables or Agent homes were changed.

## Schema and atomic claims

Migration `202608190002_collab_p4.sql` creates only `collab_boards`,
`collab_board_columns`, and `collab_cards`. It runs after the P1 and P3 migrations on an empty
schema. Completion is the stored `is_done` boolean; no column title is classified. Claim state has
the required paired `claimed_by` / `claimed_at` check and no TTL field.

The real PostgreSQL test `concurrent_card_claim_has_exactly_one_winner` starts Alice and Bob behind
the same Tokio barrier and executes the claim operation concurrently. The operation itself is one
conditional statement:

```sql
UPDATE collab_cards c
   SET claimed_by = $1, claimed_at = $2, updated_at = $2
  FROM collab_boards b
 WHERE c.id = $3 AND c.claimed_by IS NULL AND c.board_id = b.id
   AND EXISTS (... room membership ...)
RETURNING ...;
```

The test requires exactly one `claimed` result. The loser receives the serialized status
`already_claimed` and the winner's `claimedBy` id; the final card row names only that winner.

## Structured board messages and wake path

Card creation, claim, move, runtime release, and manual release allocate a room sequence and insert
a `kind=system` message in the same transaction as the card mutation. The message carries a JSON
object such as:

```json
{
  "type": "card_created",
  "boardId": "work",
  "columnId": "todo",
  "cardId": "card_…",
  "title": "Claim this card and reply P4_CARD_CLAIMED_V3",
  "assigneeId": "alice"
}
```

After commit, both daemon IPC and MCP `card` mutations publish the same three signals:

1. `MessageNotice` enters the existing 2.5-second debounce and triage/wake path;
2. `rooms_changed` refreshes the durable message stream;
3. `boards_changed { roomId }` refreshes the affected board.

The PostgreSQL test asserts consecutive structured `card_created`, `card_claimed`,
`card_claim_released`, and `card_moved` payloads and verifies that an assignment appears in the
human-readable system body. The Desktop message test separately proves that card references come
from `systemPayload.cardId`, never from parsing that prose.

## Real daemon assignment run

A real daemon and OpenCode 1.18.18 were started against the isolated schema. The setup created room
`general`, Agent `alice`, board `work`, and three explicit columns. Creating an Alice-assigned card
through daemon IPC produced sequence 3 with this durable evidence:

```text
author=user
kind=system
body=user created card “Claim this card and reply P4_CARD_CLAIMED_V3” and assigned it to alice
systemPayload.type=card_created
systemPayload.assigneeId=alice
```

The same notice produced an Alice triage row with `actionable=true`, `source=fail_open`, and
`upToSequence=3` because that isolated run intentionally had no triage model configured. After the
debounce, `agent-list` showed:

```json
{
  "id": "alice",
  "opencodeSessionId": "ses_fe5f66b0affejqZQxSD5ecEvqd",
  "activity": { "kind": "replying" }
}
```

This proves Desktop/IPC assignment reaches the normal scheduler and starts the assigned Agent's
OpenCode turn. The provider turn did not finish during the manual observation window, so this
record does **not** claim a manually observed `openwork_card` call from that run. MCP tool schema and
claim behavior are covered by the root-object schema regression plus the real PostgreSQL race.

The first manual attempt exposed an actual integration defect: representing `card` as a root enum
made rmcp reject the generated input schema with `Schema is missing 'type' field`. `CardRequest` is
now a root object with an `action` enum; a regression test asserts its generated root type is
`object`.

## Claim release

Release behavior follows the three documented paths:

- daemon startup performs one bulk conditional update, logs `daemon.startup.released_claims=N`,
  publishes board invalidations, and creates no room messages;
- a 15-second worker checks claims older than the 60-second grace period through the owning Agent
  home's instance-scoped `GET /session/status`; an idle or missing session is released with a room
  system message;
- the Desktop unlock action performs a claimant-conditional release and writes the same structured
  system-message form.

The PostgreSQL test claims a card, runs startup release, and asserts that the claim is gone while
the room message count is unchanged. It then reclaims and manually releases the card, requiring a
`card_claim_released` payload. The OpenCode client tests verify the directory header and running
classification for `busy` / `retry` versus `idle`.

## Desktop behavior

The board view lists boards, columns, cards, assignment, current claimant, and the explicit done
flag. It can create an assigned card, move it between columns, and force-release a claim. Stores
always reload daemon-canonical board state after writes.

Vitest coverage proves:

- a column titled like a done column remains unfinished when `isDone=false`;
- assigned card creation forwards `assigneeId` and reloads the room board;
- a rendered card exposes both the assignee and claimant identity;
- `boards_changed` refreshes only the named room without a manual UI reload;
- a structured card system message opens the board destination without parsing message text.

## Acceptance mapping

| Check | Evidence |
|---|---|
| Create/move creates a system message and wakes | Same-transaction PostgreSQL assertions; shared `publish_card_mutation`; real assigned-card run reached Alice `replying` |
| Two Agents race one card | Real PostgreSQL barrier test: one `claimed`, one explicit `already_claimed` with `claimedBy` |
| Desktop assignment wakes the selected Agent | Real daemon sequence 3, actionable triage row, persisted session id, and `replying` activity |
| Daemon restart releases all claims | Real PostgreSQL startup-release assertion: claim cleared, zero extra room messages |
| Claimed card updates without refresh | `boards_changed { roomId }` wire test, controller routing test, canonical board fetch, and claimant rendering test |
| Runtime/manual release retention | Claimant-conditional update; runtime/manual structured system-message assertions; startup remains log-only |
| Board versus session todo | Repaired `AGENTS.md` keeps exactly five coordination rules and adds the required one-sentence board/todo distinction |

## Gates

The final working tree passed:

```text
npm --prefix desktop run typecheck                         PASS
npm --prefix desktop test                                  PASS (73 files, 413 tests)
cargo fmt --all -- --check                                 PASS
cargo clippy --workspace --all-targets -- -D warnings      PASS
TEST_DATABASE_URL=postgres://openwork:openwork@localhost:5432/openwork cargo test --workspace
                                                            PASS
cargo tree -p openwork-collab                              PASS
```

The required workspace test ran the PostgreSQL tests rather than silently skipping them. The board
race passed in 0.33 seconds. Two unrelated live-provider tests remained ignored because their
separate API-key/network preconditions were absent; no test failed, and the known intermittent
`openwork-core` `turn not found` failure did not occur.

Among OpenWork crates, the `openwork-collab` dependency tree contains only
`openwork-models` and `openwork-credentials`. It does not contain `openwork-core` or any other
OpenWork crate.
