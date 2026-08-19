# P2 acceptance record

Date: 2026-08-19 (Asia/Shanghai)  
Host: macOS, OpenCode 1.18.18, PostgreSQL in `openwork-postgres`

All database checks below used isolated schemas. The schemas, daemon homes, and external test files
were removed after the checks.

## Shell switching and event lifetime

The local Vite UI was opened at 1280 × 720. Starting in the workbench, the **Collaboration** footer
button opened the collaboration Rail and room layout. **Return to Workbench** returned to the first
Shell. Entering collaboration again and reloading still showed **Return to Workbench**, proving that
`openwork-mode=collab` survived a reload. The non-macOS rendering put the first Rail icon at the top
without a blank inset.

The root-bridge regression test reads the actual component sources and verifies that both
`useCoreEventBridge()` and `useCollabEventBridge()` are called by `App.tsx`, while `AppShell.tsx`
does not own the core bridge. The transitive import-graph test starts at `CollabShell.tsx` and rejects
any path reaching `features/{chat,sessions,traces,models}`.

On the real Tauri build, CoreGraphics reported an onscreen `openwork-desktop` window at
`X=95, Y=49, Width=1280, Height=800`. After terminating the Desktop dev process, the separately
spawned daemon still answered `status` with OpenCode version `1.18.18` and PID `18131`, then stopped
only after an explicit `shutdown` request.

The Tauri window advertised `sharingState=0`, and macOS reported `postEventAccess=false` for the
automation process. Therefore the host would neither include the WebView in a screen capture nor
allow a synthetic mouse drag. The macOS Rail path is instead guarded by a rendering test that
requires exactly a 28px (`h-7`) top inset with `data-tauri-drag-region="deep"`; the non-macOS path
must render that inset as `hidden`. No successful native drag is claimed by this record.

## Real Agent and permission paths

An isolated daemon created Agent `alice`, room `general`, and sent:

```text
@alice Reply with exactly P2_E2E_OK using openwork_reply.
```

The durable room stream contained consecutive entries:

```text
1  user   @alice Reply with exactly P2_E2E_OK using openwork_reply.
2  alice  P2_E2E_OK
```

For approval, Alice was asked to read `/private/tmp/openwork-p2-external.txt`. The daemon's global
pending collection received an `external_directory` request attributed to Alice. Replying `once`
returned `accepted: true`, and Alice durably replied `P2_EXTERNAL_APPROVAL_OK` in the same room.

A second request for `/private/etc/hosts` was rejected with the message `Use memory instead`.
`accepted: true` was returned; Alice did not read the file and published a refusal explaining the
outside-home restriction. This proves the rejected turn changed route; it did not echo the reason
verbatim.

A fresh isolated run then produced pending permission
`per_018c1eee9001b0VYtjgXYxmYiF` for `/private/etc/*`. The Desktop-facing abort IPC returned:

```json
{"aborted": true}
```

The immediately following global pending snapshot was `[]`.

## Acceptance mapping

| P2 check | Evidence |
|---|---|
| Shell round trip and persistence | Browser steps above; `modeStore` tests |
| Workbench updates survive switching | Both bridges mounted above the Shell ternary; root-bridge test |
| Workbench receives collaboration unread | Root collaboration bridge plus sequenced daemon invalidations; controller/store tests |
| macOS traffic-light inset | Real Tauri window plus platform rendering test; native drag automation limitation recorded above |
| `@` reply in the same room | Real two-message stream above |
| Create/edit/disable Agent | Tauri command/store tests; PostgreSQL test proves disabled Agents stop mention matching while historical author identity remains |
| `once` / `reject` approval | Real OpenCode permission runs above; approval card exposes both actions and a reject-reason field |
| Unread and approval counts stay separate | Event-controller and Rail rendering tests assert distinct paths and badges |
| Abort | Real OpenCode abort result and empty pending snapshot above |
| Shell import boundary | Transitive import-graph test |

## Gates

The final working tree passed:

```text
npm --prefix desktop run typecheck                         PASS
npm --prefix desktop test                                  PASS (69 files, 404 tests)
cargo fmt --all -- --check                                 PASS
cargo clippy --workspace --all-targets -- -D warnings      PASS
TEST_DATABASE_URL=postgres://openwork:openwork@localhost:5432/openwork cargo test --workspace
                                                            PASS
cargo tree -p openwork-collab                              PASS
```

The full workspace run executed all four collaboration PostgreSQL tests. Two pre-existing live-model
tests remained ignored because they require an external API key and network access. The dependency
tree contains only `openwork-models` and `openwork-credentials` among OpenWork crates.

The Desktop package has no linter dependency or configuration. P2 briefly added a `lint` script that
was the same `tsc --noEmit` as `typecheck`; it was removed afterwards, because one check listed under
two names makes a gate list look stronger than it is. `typecheck` is the single TypeScript gate.
