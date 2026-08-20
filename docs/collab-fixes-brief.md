# 协作模式修正项 · 实施 brief

**这份文件是给实施者的工单，不是设计文档。** 语义权威是 [collaboration.md](collaboration.md)，DDL 权威是 [collaboration-data-model.md](collaboration-data-model.md)，前端权威是 [collaboration-desktop.md](collaboration-desktop.md)。三份都已按本轮结论更新，**有任何冲突以那三份为准，不以本文为准**。

> **本篇已部分被取代。** F1–F8 落地后，"一个 Agent 两个房间"暴露出唤醒粒度与投递粒度不一致的问题，唤醒随之从**按房间**改成**按 Agent 一次覆盖整个收件箱**，游标改为**逐房间结算**，并新增 `ack` 工具。受影响的是 F1（游标推进的条件）与 F5（triage 的粒度）。**以 [collaboration.md](collaboration.md) §5 / §7.3 / §8.1 / §8.2 / §8.3 为准**，本篇其余部分仍然有效。

背景：`crates/openwork-collab` 的 P1–P6 已经实现。下面 8 组是设计与实现不符、或存在结构性缺口的地方。行号是本文写作时的位置，会漂。

按 **F1 → F8** 顺序做，每组一个独立 commit。F1 是唯一在生产里持续造成损失的一条，其余可以按顺序推进。

---

## F1 · Agent 的已读游标没有任何写入者（最高优先）

### 现状

1. `storage.rs:321` 的 `mark_user_read` 是**全 crate 唯一一处** `UPDATE collab_room_members`，其 WHERE 里硬编码了 `participant_id = 'user'`。
2. 因此**没有任何路径推进 Agent 的 `last_read_seq`**，它 `DEFAULT 0` 且永远是 0。
3. `mcp.rs:219` 的 `inbox` 工具描述写死 "This tool never advances the persisted read cursor."
4. `storage.rs:563` 的 `inbox()` 查询**没有 LIMIT**，且跨该 Agent 的全部房间。
5. `scheduler.rs:789` 的 `build_wake_prompt` 把 `inbox` 整个内联进 prompt。

**合起来：每一次唤醒都把该 Agent 所在全部房间的全部历史消息完整注入一次。** 成本随房间历史线性增长。

设计里 [collaboration.md §7.3](collaboration.md) 的"谁推进"那一格此前是空的。注意这不是"照抄 §7.3 原文"就能修的——§7.3 已按本轮结论重写，**按新版实现**。

### 目标

**a. `mark_user_read` 泛化成 `mark_read(room_id, participant_id, through_sequence)`。** 现有的用户侧调用（`daemon.rs` 的 IPC）传 `"user"`。不是让 Agent 自己调——`inbox` 工具保持只读。

**b. daemon 在 run 收尾时推进 Agent 的游标。** 位置在 `scheduler.rs` 的 `handle_idle`（现在 `finish_run(&run_id, "completed", ...)` 在 `scheduler.rs:590`）。与 F2 的 outcome 派生**在同一处一次结算**。

> **这一段已被逐房间结算取代**，见 [collaboration.md §7.3](collaboration.md)。推进与否现在取决于**这个房间**有没有被发布或 `ack`，而不是整轮的 outcome；原先"连续 2 次 `unpublished` 强制推进"随之删除——它会推进一个 Agent 从未处理过的房间，正是这份 brief 要修的那类错误。下表保留作为当时的决定记录。

| run 结果 | 推进 |
|---|---|
| `acted` / `silent` | 推进到本轮投递过的最高 seq |
| `interrupted` / `failed` | 不推进 |
| `unpublished` | 不推进；同一批未读连续 2 次之后强制推进 |

**c. "投递过"不含注入。** 只有本轮 wake prompt 里实际带出去的消息算投递。运行中注入进去的消息**保持未读**，由下一轮的 wake prompt 投递、由那一轮的完成来推进。本轮投递的最高 seq 在 dispatch 时记进 `RuntimeState`。

> 这条是 §8.1 "未读绝不因已注入而清除"能逐字成立的原因。写错方向就是把那条不变量破掉，而失败形态是静默丢消息。

**d. 入房初始化。** `storage.rs:347` 的 `add_member` 把 `last_read_seq` 初始化为房间当时的 `next_seq`，不是 0。对人类与 Agent 一视同仁。这是初始化不是推进路径。

**e. `inbox()` 加 LIMIT，并返回省略标记。** 该标记必须出现在 **prompt 正文**里，不能只留在元数据——模型需要知道自己看到的是截断的。

**投递每个房间最旧的未读，不是最新的。** 游标推进到"本轮投递过的最高 seq"，所以投递集合必须是连续前缀；投递最新的 N 条会让被丢掉的旧消息落到游标下方并**永久不可达**。详见 [collaboration.md §7.3](collaboration.md)。

### 验收

- 同一房间连续两次唤醒，第二次的 prompt 不随房间历史增长；
- Agent 的 `last_read_seq` 在 run 完成后推进，且推进值不含本轮注入的消息；
- `interrupted` / `failed` 不推进；未被发布也未被 `ack` 的房间不推进；
- 一轮结束留下一条 `inbox.settled`，写明哪些房间推进了、哪些留到下一轮；
- 新成员入房后的首次唤醒，未读不含入房前的历史。

### 验证任务（请留证据）

修完后用日志抽屉对比同一房间修复前后两次 wake 的 token 用量。"房间跑得越久越慢、额度掉得越快"的根因很可能就在这里，值得留下修复前后的数字。

---

## F2 · 一轮的结果不可区分

### 现状

`scheduler.rs:590`：只要 session 走到 idle，run 一律记 `completed`，**不看有没有调过任何工具**。

于是三件完全不同的事同形：Agent 正确地闭嘴了 / 它的结论被注入吞了（R-N7）/ 它把回复当正文吐出来但没调 `reply`。

### 目标

`collab_runs` 加 `outcome` 列，由 daemon **服务端派生**，不加工具、不加 prompt、不花 token。判据 daemon 手上都有：它是全部 MCP 调用的接收方，也在消费事件流里的 assistant 正文（`activity.rs` 已在处理 `text` part）。

| 判据 | outcome |
|---|---|
| 本轮调过 `reply` / `react` / `card` | `acted` |
| 无工具调用，正文近乎为空 | `silent` |
| 无工具调用，却吐了大段正文 | `unpublished` |

DDL、两条 CHECK、以及 `unknown` 回填值的理由见 [collaboration-data-model.md §5](collaboration-data-model.md)。

**迁移注意**：`collab_runs_outcome_scope` 这条 CHECK 会在历史数据上直接失败。必须先把既有 `status = 'completed'` 的行回填成 `unknown`，再加约束。**不要**把它们编成 `acted` 或 `silent`——那些轮次当时没有记录判据，事后判不出来，编了就是凭空造事实。

### 验收

一轮无工具调用且正文近乎为空记 `silent`；无工具调用但有大段正文记 `unpublished`；两者在日志抽屉里可区分，`unpublished` 显眼。

### 不要做

- **不加 `pass` 工具**（理由见 [collaboration.md §8.3](collaboration.md)）；
- **不做 auto-relay 兜底**（不要把那段正文替 Agent 发出去）。先让 `unpublished` 跑一周看计数。

---

## F3 · wake prompt 按无状态写，但 session 是持久的

### 现状

- `scheduler.rs:658`：每次唤醒取 `glance(room_id, 50)`，**50 条完整消息**；
- `scheduler.rs:680`：`MEMORY.md` 整份 `read_to_string`，**无上限**；
- 两者每轮全量重发，而每个 Agent 的 OpenCode session 跨重启复用（一条长期存在的 transcript）。

后果是复利的：第 N 轮塞消息 1–50、第 N+1 轮塞 2–51，都留在同一条 transcript 里。压缩由 OpenCode 自己管，**我们正在制造它必须压缩的东西，而压缩本身是一次付费模型调用**。

### 目标

| 时机 | 发什么 |
|---|---|
| 常规唤醒 | 未读增量 + 名册 + promptNote |
| 观察到 `session.compacted` 之后的第一次唤醒 | 补全量：房间近况 50 条 + `MEMORY.md` |
| 该 Agent 的首次唤醒、daemon 重启后的首次唤醒 | 同上 |

信号已经在手：`activity.rs:21` 已经在消费 `session.compacted`（映射成 `AgentActivity::Compacting`），只是没被用来做这件事。给每个 Agent 维护一个"需要补全量"的标记即可。

补全量时，房间近况里已含的消息不在未读里重复第二份，未读只留 id 引用。`MEMORY.md` 保持内联但加硬上限与截断标记（**不要**改成"你自己去读文件"——记忆的用途恰恰是它自己想不起来要读）。

### 验收

`session.compacted` 之后的第一次唤醒补发全量，其余唤醒不发；`MEMORY.md` 超上限时被截断并带标记。

---

## F4 · 一轮能被无限次注入

### 现状

`scheduler.rs:38` 的 `WakeState` 只有 `{running: bool, pending_rerun: bool}`，**没有注入计数器**。只要 `running`，每个 debounce 窗口（`scheduler.rs:34`，2.5s）过去就产生一次 `Inject`。

而 P0 实测结论是：**每一次注入都可能把进行中那一轮的结论吞掉**。消息每 3 秒来一条、一轮跑 30 秒 → 被注入约 10 次，可能一句话都没发出来。表现是"人多的时候它就不吭声了"，日志里每轮却都是 `completed`。

### 目标

`WakeState` 加 `injections`，`Start` 时清零。达到 **4** 次后不再注入，只置 `pending_rerun`（加一个 `WakeAction::Defer`）。

溢出的消息本来就还是未读（注入不清未读，且按 F1-c 注入过的消息不推进游标），所以它们已经有归宿——**不需要引入任何新概念**。

上限取 4 而不是 cumora 的 8：cumora 在 hop 边界拼接、注入是安全的；这里每次注入都可能吞结论，代价更高的动作预算应当更小。

### 验收

一轮被注入 4 次后不再注入，溢出的消息由 pending rerun 的那一轮完整拿到。

---

## F5 · triage 看不见队友

### 现状

`triage/client.rs:78` 的 prompt 只有：任务描述、候选人自己的人格、房间 id、新消息。**没有名册，不知道房里还有谁，不知道谁已经醒着。**

`scheduler.rs:240` 对每个候选人独立 triage、独立唤醒。5 个 Agent 的房间，人类发一句"谁能看下这个"，5 个 triage 各自判出 actionable，5 个主推理轮同时开跑。HELD 只在**写入时**拒绝后四个，那时四轮额度已经花完了。

### 目标

给 triage 加一个信号：**活跃 run 归属于本房间的队友集合**。三条约束缺一不可：

1. **只给 triage，绝不进主唤醒 prompt。** §9.1 禁的是大脑从 composing 列表推出"我排第几"；triage 输出是二值的，没有名次可推。主 prompt 里完全适用原禁令。
2. **是集合不是序列**，不带顺序也不带时间戳。**按房间归因**——`RuntimeState` 同时持有 `active_run_id` 和 `room_id`，只收活跃 run 归属于本房间的队友，否则 A 因 R1 的活醒着会误伤 R2 的唤醒。**消息作者从集合里剔除**。
3. **有人类在等时此信号视同不存在**，triage prompt 与不带它时**一字不差**。判定放 `wake_triage` 内部（`human_waiting` 就在 `wake_triage.rs:33`），`scheduler.rs` 只传原始集合——守门判断只允许存在一处。

第 3 条是承重的：否则 A 被唤醒后正确判断"这不是叫我"（记 `silent`）、B/C 因"A 在处理"没醒，房间对着一个等着的人彻底哑火。

### 验收

有人类在等时，triage prompt 与不带该信号时逐字相同；该信号只含活跃 run 归属于本房间的队友，且不含消息作者。

---

## F6 · 房间拓扑：`dm` 工具缺失、成员变更无声、`muted` 无写入者

三处同一个形态——被生产逻辑读取，却没有任何写入者。

### 现状

- **`dm` 工具不存在。** [collaboration.md §5](collaboration.md) 的工具表列了它，`mcp.rs` 只有 5 个（`reply` / `inbox` / `glance` / `react` / `card`）。DM 房间只能从 `daemon.rs:435` 的 `CreateDirectRoom` IPC 建，而 **Desktop 侧零调用**。于是 §11.2 整套 DM 死循环探测（`decide_dm_progress` / `should_probe_agent_dm` / triage source `dm_agent_engage`）是**不可达代码**——P5 验收 #36 能过是因为测试自己调 `create_direct_room` 造了房间。
- **`storage.rs:347` 的 `add_member`** 只写一行成员表，**不发 system 消息、不唤醒任何人**；`remove_member` 根本不存在。
- **`muted`** 被四处生产查询读取（`storage/coordination.rs:29` 及 `storage/proactivity.rs` 三处），但**无 SQL 写入、无 UI**，只在 DTO 里露了个字段。

### 目标

**a. 补 `dm` MCP 工具**，语义见 §5。身份仍从 token 绑定，不接受参数里的 agent id。DM 复用靠 `direct_key`。

**b. 成员变更发 `system` 消息并进入正常唤醒流。顺序不能反**，两个方向各埋着一个 off-by-one：

| | system 消息发在哪一步 | 反过来的后果 |
|---|---|---|
| 加入 | 成员写入**之后** | 新成员还不在成员表里，收不到那条宣布它加入的消息 |
| 离开 | 移除**之前** | 当事人永远看不到那条解释它 inbox 为什么突然安静的消息 |

**c. `muted` 补写入者 + Desktop 开关。** 注意这一列**两种含义共用**：用户那一行是"别把这房间算进我的未读总数"，Agent 那一行是"这房间不要唤醒它"。两个开关都要有。后者是 Agent **不被允许自己 `leave`** 的前提。

### 不要做

**不给 Agent `invite` / `kick` / `leave`。** 拓扑归用户——把 Agent 拉进房间等于让它开始为该房间每条消息烧 triage，房间成员与 `enabled` 是用户手上仅有的两个开销闸门。

### 验收

Agent 能通过 `dm` 开启私聊，§11.2 的死循环探测在**产品路径**上可达（不只在测试里）；拉人/移除产生 system 消息并唤醒，顺序如上；Desktop 能 mute 某房间里的某个 Agent，mute 后它不再被该房间唤醒。

---

## F7 · 人格底座

### 现状

`home.rs:74` 的 `render_agents_md` 渲染的 `AGENTS.md` **只有协议**：读 inbox、调 reply、待在 home、按 id 称呼、五条协调规则。人格完全外包给用户填的 `system_prompt`。

用户不写或写得敷衍时（常态），一屋子同事全是引擎的默认助手音。

### 目标

在人格正文**之前**渲染一层所有 Agent 共享的说话规则，四条（详见 [collaboration.md §3.4](collaboration.md)）：

1. 别逐字重复上一条；会重复就 `react` 或闭嘴
2. 语言跟随对方
3. 简短，通常 1–4 句
4. 有立场，可以不同意

**底座与用户人格冲突时底座赢**，这一条明写在底座第一行。

### 不要做

**不引入"你是真人不是 AI"的角色锁**（cumora 有，这里明确不要）。**不引入表情短码指引**（依赖 cumora 客户端的渲染器，这里没有对应物）。

### 验收

用户把 `system_prompt` 留空时，Agent 仍不逐字重复上一条、语言跟随对方。

---

## F8 · 截断按字符边界

F1-e 的 LIMIT 标记与 F3 的 `MEMORY.md` 上限都会引入字符串截断。**`&s[..n]` 切在非字符边界上会 panic。** 按字符边界切，不按字节。

现有的 `activity.rs:11` `MAX_ACTIVITY_CHARS: 80` 也一并检查。

### 验收

构造超长且含多字节字符（中文、emoji）的正文与 `MEMORY.md`，截断路径不 panic。

---

## 贯穿全程（原有约束，别破坏）

- `openwork-collab` 的依赖图中不出现 `openwork-core`；
- `crates/openwork-core/migrations/` 无任何 `collab_*` 变更；
- 只使用非 `/experimental/` 的 OpenCode 端点；
- 前端不出现任何协作语义判定（HELD、去重、认领的裁决），界面只显示 daemon 给的结果；
- 时间列全部 `TIMESTAMP WITHOUT TIME ZONE` 东八区，出库字符串带 `+08:00`（见 [.claude/rules/database.md](../.claude/rules/database.md)）;
- 新增的纯函数与 store 要有与现有同强度的测试覆盖。

## 两条已接受的残留风险（不要试图在本轮"顺手解决"）

- **R-N8**：F1 的游标推进覆盖不到被注入吞掉的那一轮——那一轮几乎总以 `completed` + `acted` 收场，游标照常推进，被吞的工作失去重试触发。已知、已接受，理由是现状严格更糟。
- **R-N9**：F5 的信号在纯 Agent 房间里盖不住一个形态（A 判 `silent` 后 B/C 被压，提问要挂到停滞推动才有人接）。已知、已接受。

两条都已写进 [collaboration.md §15](collaboration.md)。
