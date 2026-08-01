# 权限系统 P5 实施 brief（最后一期）

> 交接文档，P5 落地后可删除。P1–P4 的 brief 此时也都可以删掉。
>
> 规格的唯一事实来源是 [permissions.md](permissions.md)。两者冲突时以它为准。

## 0. 一句话

**P1–P4 让越来越多的调用不再打断用户。P5 让用户在 Trace 里一眼认出哪些是这样跑掉的。**

这是一次呈现改动 —— 不新增存储、不新增字段、不改任何决策。**Rust 侧已经在交接前修好一处**（见 §1.1），其余工作在 `apps/desktop`。

## 1. 这一期为什么这么小

数据、管道、界面**都已经存在**，P1–P4 一路加下来的：

| 已有 | 位置 |
|---|---|
| 七个权限属性写入 Tool Span | `run_loop.rs` → `ToolTraceAttributesV1` |
| 属性经 wire 到前端 | `traceViewModel.ts:55-56` |
| 属性在 Span 详情里渲染 | `TurnTraceDrawer.tsx` 的 details 分组 |
| 采集完整度 | `TurnTraceDrawer.tsx:190-195` 顶部的 `SummaryPill` |

**缺的只有一件事：自动放行的 span 埋在详情里，得一条条点开才看得见。**

时间线上一行只显示状态点、图标、名称、耗时。一次会话里 `ls` / `cat` / `git status` 和 `cargo test`（用户批准过的）长得一模一样 —— 而这两类的区别正是这一期要暴露的。

### 1.1 交接前已修的一处 Rust 缺陷

`run_loop.rs` 原先把 `DecisionSource::Builtin | DecisionSource::Mode` 一起记成 `"builtin"`，于是 Trace 里**永远不会出现 `mode`** —— 用户无法区分"本来就允许"和"因为你开了 acceptEdits"，而后者正是事故复盘第一个要问的。

已改成两个独立取值，并把 `session_runtime.rs` 里那条断言 `"builtin"` 的测试改成断言 `"mode"`（它当时正好在固化这个错误）。**这不属于 P5 范围，是 P4 遗留的 §7 缺陷，先修掉是为了让 73b 真实可达。**

## 2. 做什么

### 2.1 时间线上标出四类

在 `TraceTimeline.tsx` 的行组件里，按 Tool Span 的 `permissionDecision` + `permissionDecisionSource` 分类：

| 类别 | 判据 | 为什么要单独标 |
|---|---|---|
| **自动放行** | `decision = allow` 且 `source ∈ {builtin, readonly_proof, mode, mode_fs_command, session_grant}` | 用户当场没看见它发生 |
| **静默拒绝** | `decision = deny` 且 `source = builtin` | 同样不出卡片。**它是唯一"有人试图越界"的信号**，而用户最看不见它 |
| 用户批准 | `decision = allow` 且 `source = user` | 用户当场看见过 |
| 用户拒绝 | `decision = deny` 且 `source = user` | 同上 |

**静默拒绝不要漏。** 模型写 `.git/config` 被拒之后收到一条 tool result 就换方案了，整个过程用户毫不知情。实现时很容易把它归进"没什么可标的"。

标记形式自定（色点、徽章、图标角标都行），但要求：

- 四类**互相可区分**，不是"自动/非自动"两分；
- 自动放行的那类还要能看出**来源**——`readonly_proof` 和 `mode_fs_command` 是两种很不一样的放行理由，前者是我们核对过参数，后者是模式开着；
- 悬浮或选中时显示 `readonlyProofKey`（如果有）——**判定表是我们自己背书的东西，写错时按条目反查是唯一的补救**（§7）。

### 2.2 让完整度在"有缺口"时显眼

`TurnTraceDrawer.tsx:190-195` 已经有完整度 pill，但它和"耗时""token 数"这些并排，`complete` 和 `partial` 视觉上一样。

**`partial` / `none` 必须显著。** 这是这一期唯一的安全要求，理由见 §3。

### 2.3 不做

- 不新建表、不新增 `SessionUpdate`、不做 Snapshot 折叠、不在 `SessionActor` 里存记录；
- 不做独立的"会话活动"面板；
- 不做会话授权列表（见 §4 ①）；
- **不碰 `crates/` 一行**（§1.1 那处已在交接前修完）。

## 3. 唯一的安全要求：缺口必须可见

Trace 是 best-effort，[trace.md §10](trace.md) 说得很直白：

> 队列满时 `try_send` 直接丢，批量写失败丢整批。

所以这份标记**可能少标几条**。这本来是致命的 —— **一个会漏报的漏报检测器比没有更糟**，用户扫一眼觉得没事，而漏掉的那条恰恰是出问题的那条。

救它的是完整度的算法（[trace.md §12](trace.md)）：

> **expected** 来自 `turns.tool_call_count`。这是**独立参照物**——由业务写入路径维护，**与 Trace 写入路径互不依赖**。

跑了 20 次工具调用只落了 18 条 span，`expected=20 / captured=18 / state=partial`。**丢失是能被发现的，因为"应该有多少"这个数字走的是另一条不会丢的路。**

因此契约不是"每条都在"，而是：

> **单条记录可能丢，但丢没丢一定看得出来。**

`partial` 状态如果和 `complete` 长得一样，这个契约就失效了 —— 那才是真正的假承诺。**§2.2 是本期唯一不能打折的一条。**

## 4. 两处这个方案覆盖不到的，以及为什么可以不管

**① 会话授权列表。** 授权在 `SessionActor` 内存里，不是 span。

不做。授权产生的效果在 Trace 里看得见（`source = session_grant` 的那些 span 带着 `permissionRuleId`），再单独列一遍是冗余。真要有，挂在模式指示器的悬浮层上更合适，不值得为它建面板。

**② 当场看不见。** Trace 是另一个页面，用户对话到一半不会去看。

不做。[permissions.md §5.3](permissions.md) 已经确立自动放行不出卡片是**设计意图**，要求的是"事后能查"，而 Trace 页正是查的地方。

## 5. 顺带改一句文档

[permissions.md §7](permissions.md) 现在说那些权限字段"**不是可选的观测增强，而是这套模式的配套义务**"。

这句话把保证下在了错的地方 —— 字段住在一个按设计会丢的通道里。改成把保证下在完整度上：

> 单条记录可能因 Trace 的有损写入而丢失；**保证的是丢没丢一定看得出来**——完整度的 `expected` 来自 `turns.tool_call_count` 这个独立参照物，与 Trace 写入路径互不依赖（trace.md §12）。因此界面在 `partial` / `none` 状态下必须显著提示，否则这条保证不成立。

这是一句话的措辞修正，不是迁移。**上一版 brief 里建议的专用 `permission_decisions` 表取消，不要做。**

§7 与 §6.4 / §6.5、73a–73e 已经按本 brief 的口径改完 —— **事实来源与本文现在一致，不存在需要你二选一的冲突。**

## 6. 实现顺序

| # | 内容 | 完成信号 |
|---|---|---|
| 1 | 完整度在 `partial` / `none` 时显著化 | 快照测试：两种状态的标记与 `complete` 不同 |
| 2 | 时间线四类标记 + 来源可辨 | 见 §7 |
| 3 | permissions.md §7 措辞修正 | 已在交接前完成，核对即可 |

第 1 步排在前面，是因为第 2 步会让人更信任这个界面，而**先建立信任再暴露缺口的顺序是错的**。

## 7. 怎么测

纯前端，`vitest` + `renderToStaticMarkup`，`TraceTimeline.test.tsx` 已有夹具可以扩：

```tsx
it('acc_73a marks each auto-allowed source distinctly', () => {
  // builtin / readonly_proof / mode / mode_fs_command / session_grant 五种
  // 断言互不相同，且都与 source=user 的那条不同
})

it('acc_73b surfaces the readonly proof key on the span', () => {
  // source=readonly_proof 的 span 能看到 readonlyProofKey
})

it('silent denials are marked, not hidden', () => {
  // decision=deny + source=builtin → 有独立标记
})

it('a partial trace does not look like a complete one', () => {
  // completeness.state='partial' 的渲染与 'complete' 不同，且含缺口条数
})
```

最后一条对应 §3，**不能省**。

## 8. 完成的定义

- [ ] 前端 `vitest run` 全绿；`cargo test --workspace` 全绿（这一期不该动 Rust，跑一遍确认没误伤）
- [ ] `git diff crates/` **为空**（§1.1 那处已在交接前修完，本期不应再动 Rust）
- [ ] 四类在时间线上互相可区分，自动放行的五种来源可辨
- [ ] `partial` / `none` 的完整度显著区别于 `complete`
- [ ] 三个语言包文案一致
- [ ] permissions.md §6.4 / §6.5 / §7 / 73a–73e 与实现一致（改动已在交接前完成，只需核对）
- [ ] PR 描述列出覆盖了 §9.2 的哪些编号

## 9. 最容易做错的三处

**① 漏掉静默拒绝。** 它不打断用户、不出卡片，所以最容易被当成"没什么可标的"。恰恰相反 —— 它是这个视图里唯一表示"有人试图越界"的东西。

**② 把五种自动放行来源合并成一个"自动"标记。** `readonly_proof`（我们核对过参数）和 `mode_fs_command`（模式开着，所以放行）是很不一样的两件事，合并之后用户回看时判断不了该担心哪一条。

**③ 只做标记不做完整度。** 见 §3。标记让人更信任这个界面，而一个更被信任的界面在漏报时危害更大。

## 10. 交付物

1. 代码 + 测试
2. PR 描述：验收编号覆盖清单（73a–73e）+ 确认 Rust 侧零改动

---

## 附：这是权限系统的最后一期

P5 之后 [permissions.md §9.1](permissions.md) 的分期就走完了，没有 P6。届时可以：

- 删掉 `docs/permissions-p*-brief.md` 全部五份；
- 把 §9.1 的"分期"一节压成一句话或删掉——分期表的价值在实施期间，实施完就成了历史叙事，而 [docs/README.md](README.md) 的维护原则写着**不写迁移叙事**；
- §9.2 的验收清单**保留**，它是持续有效的约束，不是过程记录。
