# 协作模式：补住新架构的保证 · 实施 brief

**这是给实施者的工单。** 语义权威是 [collaboration.md](collaboration.md)，DDL 权威是 [collaboration-data-model.md](collaboration-data-model.md)。有冲突以那两份为准。

**这一轮不改行为，只补测试**，外加两处为了能测而必需的最小 seam 调整。任何"顺手优化"都不要做。

---

## 0. 你需要先知道的架构（当前状态，已合并）

`crates/openwork-collab` 的协作模式里，一个 Agent 是一个常驻同事，推理跑在外部 `opencode serve` 里。关键点：

1. **一个 Agent 只有一条 OpenCode session**，不管它在几个房间里。
2. 因此**唤醒的单位是 Agent 不是房间**：一次唤醒覆盖它整个待处理收件箱，wake prompt 按房间分组（`rooms: [{roomId, roster, unread}]`）。triage 也是一次判完整个收件箱。
3. **已读游标逐房间结算**：一个房间只有在这一轮被**发布过**（`reply` / `react` / `card`）或被**显式 `ack`**，它的 `last_read_seq` 才前进。只是被展示过的房间保持未读，下一轮重新送达。

第 3 条是整套设计的承重墙。它存在的理由是一个真实发生过的 bug：早先的实现把"本轮投递过的所有房间"一起推进游标，于是一次专注在房间 A 的回合会把房间 B 划掉，B 随后的唤醒拿到空收件箱、什么也产不出，而日志里看着像"它判断这事不该它插嘴"——**消息被静默丢掉，且现场看不出来**。

---

## 1. 为什么要补这一轮

第 3 条的保证目前**只在数据结构那一层被断言过**。

`crates/openwork-collab/tests/observation.rs` 里的 `a_room_that_was_only_shown_never_settles` 断言的是 `RunEvidence::settled_rooms()` 算得对。但没有任何测试断言 `scheduler.rs` 的 `settle_read_cursor` **真的只对被结算的房间调 `mark_read`**。

也就是说：如果有人把 `settle_read_cursor` 里的过滤条件删掉，丢消息的 bug 原样回来，而**全部 107 个测试照样绿**。

这一轮补三条测试把这条链焊死。

---

## T1 · 只回复房间 A 时，房间 B 必须保持未读（最重要）

### 先做一个 seam 调整

`settle_read_cursor` 现在的签名收 `&DispatchServices<'_>`，那是一个捆了 8 个引用的结构（storage、homes、tokens、connections、coordination、runtime、subscriptions、observations）。而它**实际只用到其中两个**：`services.storage` 和 `services.observations`。

在测试里构造完整的 `DispatchServices` 需要一个 OpenCode 连接，纯属噪声。**把签名改窄**，直接收 `&CollabStorage` 与 `&ObservationSink`，调用点从 `services` 里取那两个字段传进去。

这不是重构练习：一个函数声明自己需要 8 样东西、实际只用 2 样，测试成本就是那 6 样的构造成本，而它们和被测行为毫无关系。

### 测试本身

库内测试（`scheduler.rs` 的 `mod tests`），走真实 Postgres。**`wake_triage.rs` 里的 `agent_dm_defaults_to_engage_then_stops_at_the_eighth_no_progress_message` 是现成的模板**：建临时 schema、`CollabStorage::from_pool`、`migrate()`。照抄那套。

步骤：

1. 建两个房间 `alpha` / `beta`，让 Agent 与 `user` 都入伙；
2. 两个房间各发几条 `user` 消息；
3. `inbox(agent)` 取到两房间的未读，算出各自的最高 seq（即 `delivered_sequences`）；
4. 构造一份 `RunEvidence`，**只标记 `alpha` 被发布过**（走 `ObservationSink::discarding()` + `set_active_run` + `mark_action(agent, "alpha")`，再 `take_run_evidence`）；
5. 调 `settle_read_cursor`；
6. 断言：
   - `alpha` 的 `last_read_seq` == 第 3 步算出的 alpha 最高 seq；
   - **`beta` 的 `last_read_seq` 原地不动**；
   - 再次 `inbox(agent)`，**beta 的原消息仍在返回里，内容一致**；alpha 的不在。

第 6 条第三点是关键——只断言游标数字不够，要断言"下一轮它真的还能看到那条消息"。

### 再加一条：`ack` 走同一条路

同样的场景，改成对 `beta` 调 `mark_ack`。断言 `beta` 的游标前进了，且轮次 `outcome` 仍是 `Silent`（站下不是发言）。

### 验证测试有牙

写完之后，**临时把 `settle_read_cursor` 里跳过未结算房间的那个分支删掉，确认 T1 失败**，然后改回来。一条删掉被测逻辑仍然通过的测试没有价值。这一步请在报告里说明结果。

---

## T2 · 一次跨房间裁决要给每个房间各记一条 `collab_triages`

`wake_triage::evaluate` 现在一次判完整个收件箱，但 `collab_triages` 仍然按 `(agent_id, room_id, up_to_seq)` 逐房间记录——这样日志抽屉才能回答"关于这个房间、截到这条消息，当时判了什么"。

测试：构造两个房间的 pending batch 一起传给 `evaluate`，用一个假的 triage HTTP server（**`wake_triage.rs` 现有测试里的 `Router::new()` 就是模板**）返回 `actionable=true`，然后断言：

- `collab_triages` 有**两行**；
- 每一行的 `up_to_seq` 是**该房间自己**的最高 seq，不是全局最高；
- 两行的 `source` 都是 `support_model`。

第二点是最容易写错的地方：一次裁决、一个 `up_to_seq` 变量，很容易被两行共用。

---

## T3 · 按 Agent 的 debounce 窗口不能被后续消息延长

`scheduler.rs` 里 `deadlines` 现在按 Agent 计（不再按 `(Agent, 房间)`），且窗口由**第一条**待处理消息起算、后续消息**不延长**它。

理由：按房间计时的旧写法里每条新消息都覆盖 deadline。改成按 Agent 之后如果还沿用覆盖语义，一个持续活跃的房间会把这个 Agent 的整个窗口无限往后推——**它永远醒不过来**，而且没有任何报错。

### 先做一个 seam 调整

武装逻辑现在是内联在 `tokio::select!` 分支里的一行 `deadlines.entry(...).or_insert_with(...)`，在 `select!` 里没法测。**抽成一个小的纯函数**，形如：

```
fn arm_debounce(deadlines: &mut HashMap<String, Instant>, agent_id: &str, now: Instant)
```

调用点改成调它。不要顺手改变语义。

### 测试

- 同一个 Agent 连续武装三次，`now` 依次递增；断言最终 deadline == **第一次** 的 `now + DEBOUNCE`；
- 两个不同 Agent 各自独立，互不影响；
- deadline 被 `remove` 之后再武装，从新的 `now` 重新起算。

---

## 怎么跑

集成测试**没有 `TEST_DATABASE_URL` 时会直接 `return`（静默跳过，显示为 passed）**。这是个陷阱：不带这个变量跑，你会看到一片绿，但 T1/T2 根本没执行。

```bash
TEST_DATABASE_URL="postgres://openwork:openwork@localhost:5432/openwork" cargo test -p openwork-collab
```

数据库没起的话：`docker compose up -d postgres`。

合并前全部要干净：

```bash
cargo fmt --all -- --check && cargo clippy --workspace --all-targets && cd desktop && pnpm vitest run
```

---

## 明确不要做

| 不要 | 理由 |
|---|---|
| 给未结算的房间加"自动结算"或超时推进 | 那就是这套设计要修掉的 bug。房间没被处理就该保持未读 |
| 因为 `carriedOver` 非空就去"修" | 那是设计中的正常状态，见 [collaboration.md](collaboration.md) R-N10 |
| 动 P7（循环硬顶 / 额度闸） | 它依赖真实环境量到的消耗数字，那一步还没做 |
| 改 `settle_read_cursor` / `evaluate` / `arm_debounce` 的**行为** | 这一轮只补测试；两处 seam 调整仅限于收窄签名与抽出函数 |
| 把 DM 死循环探测并进通用裁决 | 它问的是另一个问题，有自己的 source |

---

## 交付时请报告

1. 三组测试各自的通过情况；
2. **T1 的"验证测试有牙"那一步的结果**——删掉过滤分支后它是否真的失败；
3. 跑测试时用的 `TEST_DATABASE_URL` 是否设置了（用于确认集成测试真的执行了，而不是被跳过）。
