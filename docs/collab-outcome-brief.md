# 修正轮次结果分类器 · 实施 brief

**这是给实施者的工单。** 语义权威是 [collaboration.md](collaboration.md) §8.3 与 §7.3。

改动很小——判据一行、测试若干、文档一处。但它修的是一个**已经在真实环境里被证实的缺陷**，证据见 [collab-field-report.md](collab-field-report.md)。

---

## 1. 缺陷

`collab_runs.outcome` 有三档（[collaboration.md §8.3](collaboration.md)）：

| 判据 | outcome | 含义 |
|---|---|---|
| 调过 `reply` / `react` / `card` | `acted` | 正常 |
| 无工具调用，正文近乎为空 | `silent` | 主动闭嘴，或 triage 醒错了人 |
| **无工具调用**，却吐了大段正文 | `unpublished` | **异常**：话说进了真空 |

`unpublished` 是给 R-N7（注入吞掉结论）和"模型忘了调工具"做的**检出器**，日志抽屉里红色高亮。

`crates/openwork-collab/src/observation.rs` 的 `RunEvidence::outcome()` 实际是这么判的：

```rust
if !self.acted_rooms.is_empty() {
    RunOutcome::Acted
} else if self.text_parts.values().sum::<usize>() > MAX_SILENT_ASSISTANT_CHARS {
    RunOutcome::Unpublished
} else {
    RunOutcome::Silent
}
```

**`ack` 是一次工具调用，但它只填 `acked_rooms`、不填 `acted_rooms`。** 于是"调了 `ack`，并顺便用一句话解释为什么不接"被判成 `unpublished`——一个正常行为被标成异常。

同一份文档的 §8.3 里已经写明了正确语义：

> **`ack` 不算发布。** 只调了 `ack` 的一轮结算了那些房间，但轮次结果仍是 `silent`——它确实什么都没说。

代码没做到这一条。

### 真实环境的证据

2026-08-20 的取证（[collab-field-report.md §1.3](collab-field-report.md)）：

```
message / acted        25
message / unpublished  20      ←
agenda  / unpublished   1
rerun   / silent        1
```

**47 条 run 里 21 条判成 `unpublished`，占 45%。** 其中包含 M3 里 Alice 那 10 次纯 `ack`、M2 里 `reply` 被 HELD 之后改用 `ack` 结算的几次、以及那条 `ack` 了两个房间的 agenda run。

45% 的"异常率"意味着这个检出器已经废了：真正的 R-N7 形态会淹没在噪声里，而红色高亮会被当成背景色。

### 为什么现有测试没拦住

`tests/observation.rs` 的 `an_ack_settles_its_room_without_being_recorded_as_a_response` 断言了 ack-only → `Silent`，**并且通过**。它之所以通过，是因为那个测试**没有喂任何正文**。真实的 Agent 在 `ack` 之后几乎总会说一句话。

测试覆盖了名义场景，没覆盖真实场景。修的时候要把这个洞一起堵上。

---

## 2. 改法

### 2.1 判据

在 `acted` 与正文判断之间插一层：**`ack` 也是工具调用，它表示这一轮是刻意站下的，不是把话说进了真空。**

```
acted_rooms 非空          → Acted
否则 acked_rooms 非空     → Silent      ← 新增这一层
否则 正文超过阈值          → Unpublished
否则                      → Silent
```

期望行为：

| 这一轮做了什么 | outcome |
|---|---|
| 在某房间 `reply` / `react` / `card`（无论有没有正文） | `acted` |
| 只 `ack`，没有正文 | `silent` |
| **只 `ack`，并说了一段话** | **`silent`**（当前错判为 `unpublished`） |
| 一个房间 `reply`、另一个 `ack` | `acted` |
| 一个工具都没调，说了一段话 | `unpublished` |
| 一个工具都没调，也没说话 | `silent` |

### 2.2 只有"结算类"工具算数，读取类不算

`glance` 与 `inbox` 是**读**，不是结果。一轮只 `glance` 了几次然后吐一段话就停，那正是 `unpublished` 要抓的形态——**不要**把它们也算进来。

`dm` 只开房间、不发布内容，也不算。

判据只认两类：**结算房间的**（`reply` / `react` / `card` → acted）和**显式站下的**（`ack` → silent）。

### 2.3 不要动的

- `MAX_SILENT_ASSISTANT_CHARS`（当前 8）保持不变。阈值合不合适是另一个问题，这一轮不碰；
- `settled_rooms()` 是对的（acted ∪ acked），不要改；
- `mark_ack` / `mark_action` 的签名和调用点都不要动；
- 日志抽屉对 `unpublished` 的红色高亮**保留**——判据修好之后它会重新变得罕见，红色才有意义。

---

## 3. 测试

### 3.1 补上漏掉的真实场景

现有的 `an_ack_settles_its_room_without_being_recorded_as_a_response` 保留，**另加**一条喂了正文的：`ack` + 一段 assistant 正文 → `Silent`。

这条测试在改判据之前**必然失败**。先写它、跑一次看它红，再改判据。**报告里请写明这一步的结果**——一条在修复前就通过的回归测试是没有意义的。

### 3.2 加一张判据全表

按 §2.1 的六行做一个表驱动测试，把每一种组合都钉住。这个缺陷的根源是"判据有三档，但实际输入维度有两个（哪些工具、有没有正文）"，逐例枚举能防止下一次再漏一格。

正文用真实一点的长度（几十个字符），不要贴着阈值写。

### 3.3 跑法

```bash
TEST_DATABASE_URL="postgres://openwork:openwork@localhost:5432/openwork" cargo test -p openwork-collab
```

**不带 `TEST_DATABASE_URL` 时集成测试会静默跳过并显示为 passed。** 请在报告里写明这个变量设了没有。

合并前：

```bash
cargo fmt --all -- --check && cargo clippy --workspace --all-targets && cd desktop && pnpm vitest run
```

---

## 4. 文档同步

`collaboration.md` §8.3 的判据表把 `silent` 与 `unpublished` 两行都写成"无工具调用"，`ack` 落在哪一行只能从后文的散文里推出来。**把 `ack` 显式写进那张表**，让表本身就是完整判据。

散文里那句"`ack` 不算发布……轮次结果仍是 `silent`"是对的，保留。

---

## 5. 顺带一处报告修正

[collab-field-report.md](collab-field-report.md) §7「未能测到的内容」列了 4 项，**漏了一项**：M3 全程 digest 都只有 1 个房间（见该报告 §4.1 的表），所以 R-N10 关心的"多房间下 `carriedOver` 逐轮累积"这个形态**从未被构造出来**。

M3 回答了"模型会不会调 `ack`"（会，10/10），但没有回答"没被结算的房间会不会堆积"。请在 §7 的表格里补一行，措辞按事实写，不要改动 §4 已有的结论——那些结论本身的措辞是克制且正确的。

---

## 6. 明确不要做

| 不要 | 理由 |
|---|---|
| 把 `glance` / `inbox` / `dm` 也算作工具调用 | 读取和开房间不是结果，见 §2.2 |
| 调整 `MAX_SILENT_ASSISTANT_CHARS` | 阈值是另一个问题，这一轮不碰 |
| 取消 `unpublished` 这一档，或去掉红色高亮 | 判据修好之后它恢复成有效检出器 |
| 让 `ack` 也填 `acted_rooms` | 那会让"只站下的一轮"报成 `acted`，把 §8.3 的语义反过来破一次 |
| 回头去重算取证报告里那 21 条的新分类 | 那个库已经删了，重算不了；报告如实记录当时的观测即可 |
| 顺手实现 P7 额度闸 | 不在本轮范围 |

---

## 7. 交付时请报告

1. §3.1 那条新测试**在改判据之前是否真的失败**；
2. 判据全表测试的通过情况；
3. 跑测试时 `TEST_DATABASE_URL` 设了没有；
4. `cargo fmt` / `clippy` / Desktop Vitest 的结果。
