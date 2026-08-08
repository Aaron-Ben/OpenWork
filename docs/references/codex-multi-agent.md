# Codex Multi-Agent 项目结构详解

**这是外部参考资料，不是 OpenWork 的设计文档。** 它描述 OpenAI Codex（`codex-rs`）如何实现多代理，用于给 OpenWork 的多智能体设计提供可对照的先例。OpenWork 自己的边界、不变量和验收标准写在 [architecture.md](../architecture.md) 与 [session-runtime.md](../session-runtime.md)，**以那两篇为准**——本篇的任何结论都不构成对 OpenWork 的约束。

> 基于 codex-rs 源码通读整理。文件路径相对 `codex-rs/`，行号为阅读时的实际位置，上游演进后可能漂移。

---

## 0. 一句话定位

Codex 的多代理不是一个"多代理框架"，而是**在既有的 Thread（会话线程）基础设施上加了一层控制平面 `AgentControl`，外加一套 mailbox 通信协议和一组工具**。子代理复用了 100% 的会话运行时——同一个 `Session`、同一个 turn 循环、同一套工具、同一份 rollout 机制。没有任何"子代理专用运行时"。

这个判断很重要，因为它决定了整套代码的形状：**多代理的代码量集中在"身份、限额、通信、工具面"四件事上，运行时本身一行没改。**

---

## 1. 模块地图

```
codex-rs/
├── protocol/                              ← 类型定义层（无逻辑）
│   ├── src/agent_path.rs                     AgentPath：层级身份
│   └── src/protocol.rs                       SubAgentSource / AgentStatus /
│                                             MultiAgentVersion / InterAgentCommunication
│
├── agent-graph-store/                     ← 持久化父子边（独立 crate）
│   ├── src/store.rs                          AgentGraphStore trait（存储中立）
│   ├── src/types.rs                          ThreadSpawnEdgeStatus { Open, Closed }
│   └── src/local.rs                          SQLite 实现（走 codex-state）
│
├── core/src/
│   ├── agent/                             ← 控制平面（多代理的心脏）
│   │   ├── control.rs                        AgentControl：spawn / 投递 / 状态 / 拓扑
│   │   ├── control/spawn.rs                  spawn 全流程 + fork 裁剪
│   │   ├── control/execution.rs              AgentExecutionLimiter：并发活跃度限额
│   │   ├── control/residency.rs              V2Residency：LRU 卸载空闲子线程
│   │   ├── control/legacy.rs                 V1 兼容路径
│   │   ├── registry.rs                       AgentRegistry：路径树 + 昵称池 + 总数限额
│   │   ├── role.rs                           角色 = config layer
│   │   ├── builtins/{explorer,awaiter}.toml  内置角色的配置覆盖
│   │   ├── agent_names.txt                   昵称池（科学家名字）
│   │   ├── agent_resolver.rs                 target 字符串 → ThreadId
│   │   └── status.rs                         EventMsg → AgentStatus 映射
│   │
│   ├── agent_communication.rs             ← 通信的 OTel 埋点 + Kind 枚举
│   ├── session_prefix.rs                  ← 三种消息信封的格式化入口
│   ├── context/
│   │   ├── inter_agent_message.rs            MESSAGE / NEW_TASK 信封
│   │   ├── inter_agent_completion_message.rs FINAL_ANSWER 信封
│   │   └── subagent_notification.rs          V1 的 <subagent_notification> 信封
│   │
│   ├── session/
│   │   ├── input_queue.rs                    mailbox：投递、排空、活动通知
│   │   ├── multi_agents.rs                   MultiAgentMode 决策 + usage hint 选取
│   │   └── mod.rs:1918                       V2 子代理完成回传父代理
│   │
│   ├── tools/
│   │   ├── spec_plan.rs:886                  add_collaboration_tools：工具注册门控
│   │   ├── handlers/multi_agents_spec.rs     所有工具的 JSON Schema + 描述文本（890 行）
│   │   ├── handlers/multi_agents_common.rs   两代共用的参数解析 / 错误映射 / config 构建
│   │   ├── handlers/multi_agents/            V1 handlers（5 个）
│   │   └── handlers/multi_agents_v2/         V2 handlers（6 个）
│   │
│   ├── rollout_budget.rs                  ← 整棵代理树共享的 token 记账
│   └── thread_rollout_truncation.rs       ← fork_turns=N 的 turn 边界裁剪
│
├── tui/src/multi_agents.rs                ← 970 行渲染逻辑
└── app-server/                            ← thread/list 按 parentThreadId 过滤
```

**代码量分布**（不含测试）：

| 区域 | 行数 | 占比 |
|---|---:|---|
| 工具 Schema 与描述文本 | ~890 | 最大单文件 |
| 控制平面（agent/ 全部） | ~2000 | 核心逻辑 |
| 工具 handlers（V1+V2） | ~1400 | 胶水层 |
| TUI 渲染 | ~970 | 展示 |
| 测试 | ~9000 | multi_agents_tests.rs 单文件 4538 行 |

值得注意：**Schema 描述文本是最大的单个文件**。codex 把大量"什么时候该派工、怎么划分任务"的策略写在了工具描述里，而不是代码里。这是 prompt engineering 的重量级投入。

---

## 2. 身份系统：AgentPath

### 2.1 类型定义

`protocol/src/agent_path.rs:15`

```rust
pub struct AgentPath(String);

impl AgentPath {
    pub const ROOT: &str = "/root";
    pub const MORPHEUS: &str = "/morpheus";   // 特殊保留身份
    const ROOT_SEGMENT: &str = "root";        // "root" 是保留段名，不能作为 task_name
}
```

它是一个 newtype 包装的字符串，`serde(try_from = "String")` 保证反序列化时也走校验。核心方法只有三个：

| 方法 | 语义 | 用途 |
|---|---|---|
| `join(name)` | `/root/a` + `b` → `/root/a/b` | spawn 时构造子路径 |
| `resolve(ref)` | 相对名或绝对路径 → 绝对路径 | 工具参数 `target` 解析 |
| `name()` | `/root/a/b` → `b` | UI 显示短名 |

`resolve` 的设计很聪明：**同一个字符串既可以是相对名也可以是绝对路径**。

```rust
// agent_path.rs:59
pub fn resolve(&self, reference: &str) -> Result<Self, String> {
    if reference == Self::ROOT { return Ok(Self::root()); }
    if reference.starts_with('/') { return Self::try_from(reference); }  // 绝对
    validate_relative_reference(reference)?;
    Self::from_string(format!("{self}/{reference}"))                     // 相对
}
```

这直接支撑了工具描述里那句话：

> 如果你的任务是 `/root/task1`，spawn 一个 `task_3`，它的规范名是 `/root/task1/task_3`。你可以用 `task_3` 或全名指代它。但 `/root/task2/task_3` 只能用全名和它通信。

**兄弟之间用相对名会解析错**——这是路径体系的必然结果，codex 选择在工具描述里明说而不是加一层名字解析。

### 2.2 身份如何附着到线程

`protocol/src/protocol.rs:2843`

```rust
pub enum SubAgentSource {
    Review,                    // 内部用途：代码审查
    Compact,                   // 内部用途：压缩
    ThreadSpawn {              // ← 多代理用这个
        parent_thread_id: ThreadId,
        depth: i32,
        agent_path: Option<AgentPath>,
        agent_nickname: Option<String>,
        #[serde(alias = "agent_type")]
        agent_role: Option<String>,
    },
    MemoryConsolidation,
    Other(String),
}
```

`SessionSource::SubAgent(...)` 是线程创建时就固定的字段，之后只读。这带来一个重要性质：**任何拿到 `TurnContext` 的代码都能立刻判断"我是不是子代理、我的父亲是谁、我在第几层"**，不需要查注册表。工具门控、usage hint 选择、并发限额判定全都基于它。

注意 `agent_role` 带 `alias = "agent_type"`——字段改过名，旧 rollout 还能读。

### 2.3 注册表：AgentRegistry

`core/src/agent/registry.rs:24`

```rust
pub(crate) struct AgentRegistry {
    active_agents: Mutex<ActiveAgents>,
    total_count: AtomicUsize,
}

struct ActiveAgents {
    agent_tree: HashMap<String, AgentMetadata>,   // agent_path → 元数据
    thread_paths: HashMap<ThreadId, String>,      // thread_id → agent_path（反查）
    used_agent_nicknames: HashSet<String>,
    nickname_reset_count: usize,
}
```

**双索引**：路径查线程（工具的 `target` 解析要用），线程查路径（事件渲染要用）。两个 map 的一致性由 `register_spawned_thread` 手工维护，里面有一段处理"同一路径换了线程 / 同一线程换了路径"的清理逻辑（`registry.rs:192-203`）。

**昵称池**（`registry.rs:205`）：从 `agent_names.txt` 里随机取一个没被用过的科学家名字（Euclid、Archimedes、Newton……）。用光了就清空重来，并给名字加序数后缀——`Newton the 2nd`、`Euler the 3rd`。序数后缀的英文规则（11/12/13 用 th）也实现了。这纯粹是给用户看的可读标识，不参与寻址。

### 2.4 RAII 预留：SpawnReservation

这是整个多代理里最值得学的一个模式（`registry.rs:297`）：

```rust
pub(crate) struct SpawnReservation {
    state: Arc<AgentRegistry>,
    active: bool,
    reserved_agent_nickname: Option<String>,
    reserved_agent_path: Option<AgentPath>,
}

impl SpawnReservation {
    pub(crate) fn commit(mut self, agent_metadata: AgentMetadata) {
        self.reserved_agent_nickname = None;
        self.reserved_agent_path = None;
        self.state.register_spawned_thread(agent_metadata);
        self.active = false;      // ← 关键：标记已提交，Drop 时不再回滚
    }
}

impl Drop for SpawnReservation {
    fn drop(&mut self) {
        if self.active {
            if let Some(path) = self.reserved_agent_path.take() {
                self.state.release_reserved_agent_path(&path);
            }
            self.state.total_count.fetch_sub(1, Ordering::AcqRel);
        }
    }
}
```

spawn 是个长流程（要建线程、要 fork 历史、要写数据库），中间任何一步 `?` 提前返回，名额和路径都会被 Drop 自动归还。**不需要在每个错误分支写清理代码**，也不会因为漏写一个分支导致名额泄漏。

同样的模式在 `V2ResidencySlot`（residency.rs:28）和 `AgentExecutionGuard`（execution.rs:20）里各出现一次。

配套的原子计数用 CAS 循环而不是简单 `fetch_add` 后检查（`registry.rs:278`）：

```rust
fn try_increment_spawned(&self, max_threads: usize) -> bool {
    let mut current = self.total_count.load(Ordering::Acquire);
    loop {
        if current >= max_threads { return false; }
        match self.total_count.compare_exchange_weak(
            current, current + 1, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return true,
            Err(updated) => current = updated,
        }
    }
}
```

避免了"先加再判断超了再减"导致的瞬时超额。

### 2.5 持久化拓扑：agent-graph-store

独立 crate，只做一件事——存 parent→child 有向边。

```rust
// agent-graph-store/src/store.rs:17
pub trait AgentGraphStore: Send + Sync {
    fn upsert_thread_spawn_edge(&self, parent: ThreadId, child: ThreadId,
                                status: ThreadSpawnEdgeStatus) -> Future<()>;
    fn set_thread_spawn_edge_status(&self, child: ThreadId,
                                    status: ThreadSpawnEdgeStatus) -> Future<()>;
    fn list_thread_spawn_children(&self, parent: ThreadId,
                                  status_filter: Option<...>) -> Future<Vec<ThreadId>>;
    fn list_thread_spawn_descendants(&self, root: ThreadId,
                                     status_filter: Option<...>) -> Future<Vec<ThreadId>>;
}
```

状态只有 `Open | Closed` 两种（`types.rs:7`）。文档注释里有一条容易忽略的语义规定：

> `status_filter` 作用于**遍历经过的每一条边**，不只是返回的后代。`Some(Open)` 只走 open 边，所以一个 closed 边下面的后代不会被返回，哪怕它们自己的入边是 open。

实现 `LocalAgentGraphStore`（`local.rs:12`）委托给 `codex-state` 的 SQLite runtime。为什么要单独抽一个 trait 而不是直接用 SQLite？因为 rollout（会话历史）存在文件里，这个图必须能独立查询——恢复会话时需要**在不加载任何线程运行时的前提下**知道有哪些后代。

这就是 `restore_v2_agent_metadata`（`control/spawn.rs:128`）做的事：

```
读 agent_graph_store 的 open 后代列表
  → 对每个后代读 stored_thread 元数据（include_history: false）
  → 重新在 registry 里注册身份（reserve → commit）
  → 但不启动它们的运行时
```

恢复的是**编制**，不是进程。真要给某个子代理发消息时，`ensure_v2_agent_loaded`（`spawn.rs:250`）才按需把它 resume 起来。

---

## 3. 控制平面：AgentControl

`core/src/agent/control.rs:97`

```rust
#[derive(Clone, Default)]
pub(crate) struct AgentControl {
    session_id: SessionId,                        // 整棵树共享
    manager: Weak<ThreadManagerState>,            // ← Weak，防循环引用
    state: Arc<AgentRegistry>,
    v2_residency: Arc<V2Residency>,
    agent_execution_limiter: Arc<AgentExecutionLimiter>,
    rollout_budget: Arc<RolloutBudget>,
}
```

### 3.1 生命周期与共享范围

注释说得很清楚（`control.rs:90-95`）：

> 一个 `AgentControl` 实例最多在每个根线程/会话树创建一次。同一个实例随后被从该根 spawn 出的每个子代理共享，这让注册表的作用域限定在那棵根树，而不是整个 `ThreadManager`。

所以内部的 `Arc<...>` 字段全是**整棵树共享的可变状态**：注册表、驻留 LRU、并发计数、token 预算。`Clone` 是廉价的（都是 Arc 克隆），子代理创建时直接 `self.clone()` 传下去。

`manager: Weak<ThreadManagerState>` 用 Weak 是为了打断这条引用环：

```
ThreadManagerState → CodexThread → Session → SessionServices → AgentControl → ThreadManagerState
```

每次要用都得 `upgrade()`（`control.rs:601`），失败就返回 `"thread manager unavailable"`。

### 3.2 对外能力清单

| 方法 | 位置 | 作用 |
|---|---|---|
| `spawn_agent_with_communication` | spawn.rs:233 | V2 主入口：带 mailbox 消息 spawn |
| `spawn_agent_with_metadata` | spawn.rs:217 | V1 主入口：带 UserInput spawn |
| `send_input` | control.rs:143 | 投 UserInput（V1） |
| `send_inter_agent_communication` | control.rs:171 | 投结构化消息（V2） |
| `interrupt_agent` | control.rs:247 | 发 `Op::Interrupt` |
| `get_status` / `subscribe_status` | control.rs:277 / 345 | 单次查 / watch 订阅 |
| `resolve_agent_reference` | control.rs:325 | `target` 字符串 → ThreadId |
| `list_agents` | control.rs:383 | 列活跃代理（可按路径前缀过滤） |
| `list_live_agent_subtree_thread_ids` | control.rs:305 | 自己 + 所有后代（级联关闭要用） |
| `format_environment_context_subagents` | control.rs:361 | 生成注入上下文的子代理清单 |
| `ensure_v2_agent_loaded` | spawn.rs:250 | 按需 resume 已卸载的子代理 |
| `restore_v2_agent_metadata` | spawn.rs:128 | 会话恢复时重建编制 |

### 3.3 错误即清理

`handle_thread_request_result`（`control.rs:258`）包住所有对线程的请求：

```rust
if result.is_err_and(|err| matches!(err.details(), CodexErrorDetails::InternalAgentDied)) {
    let _ = state.remove_thread(&agent_id).await;
    self.forget_v2_residency(agent_id);
    self.state.release_spawned_thread(agent_id);
}
```

发现代理已死就顺手清三处状态：线程表、驻留 LRU、注册表名额。**用"下一次访问时清理"代替"监听死亡事件清理"**，少一个后台任务，也不会漏。

---

## 4. Spawn 完整流程

以 V2 为例。入口是工具 handler `tools/handlers/multi_agents_v2/spawn.rs:39`。

### 4.1 工具层（handler 做的事）

```
1. 解析参数 SpawnAgentArgs { message, task_name, agent_type?, model?,
                             reasoning_effort?, service_tier?, fork_turns? }
2. fork_mode() 把 fork_turns 字符串翻译成枚举：
      "none"  → None
      "all"   → Some(FullHistory)      ← 默认
      "3"     → Some(LastNTurns(3))
      fork_context 出现 → 直接报错（V1 参数，V2 不支持）
3. build_agent_spawn_config(base_instructions, turn)
      从父的 effective config 出发，刷新 turn 上的运行时字段
      （model / reasoning / approval policy / sandbox / cwd）
4. 若是全量 fork → 拒绝 agent_type 覆盖（不能既继承全部历史又换角色）
5. apply_requested_spawn_agent_model_overrides   模型/推理档覆盖
6. apply_spawn_agent_role                        角色 config layer
7. apply_spawn_agent_service_tier
8. apply_spawn_agent_runtime_overrides
9. thread_spawn_source(...)  → parent_path.join(task_name) 得到子路径
10. communication_from_tool_message(...)  构造 InterAgentCommunication
11. agent_control.spawn_agent_with_communication(...)
12. emit_sub_agent_activity(Started)      → TUI 渲染
13. telemetry counter "codex.multi_agent.spawn" { role, version=v2 }
14. 返回 { task_name } 或 { task_name, nickname }
```

第 3 步的 doc comment 值得一读（`multi_agents_common.rs:170`）：

> 跳过这个 helper 直接克隆陈旧的 config 状态，会让子代理带着错误的 provider 或运行时策略出发。

这是踩过坑留下的注释。

### 4.2 控制平面层（`spawn_agent_internal`，spawn.rs:363）

```
 1. upgrade() 拿 ThreadManagerState
 2. effective_multi_agent_version_for_spawn(...)   决定这次用 V1 还是 V2
 3. ensure_execution_capacity(...)                 并发活跃度检查（V2 且是子代理才查）
 4. reserve_v2_residency_slot(...)                 驻留名额（不够就 LRU 卸载）
 5. reserve_spawn_slot(max_threads)                总数名额（RAII）
 6. 收集继承项：turn environments、exec policy
 7. prepare_thread_spawn(...)                      预留 path + nickname，构造 SessionSource
 8. 分支建线程：
      有 fork_mode → spawn_forked_thread(...)      裁剪父 rollout 后建
      无 fork_mode → spawn_new_thread_with_source  空历史建
 9. reservation.commit(metadata)                   ← 名额落定
10. residency_slot.commit(thread_id)
11. emit_subagent_session_started(...)             analytics
12. state.notify_thread_created(thread_id)         通知 app-server 客户端订阅
13. persist_thread_spawn_edge_for_source(...)      写 agent-graph-store
14. 投递初始输入：
      UserInput             → send_input_after_capacity_check
      InterAgentCommunication → send_inter_agent_communication_after_capacity_check
15. 仅 V1：maybe_start_completion_watcher(...)     起 detached watcher
16. 返回 LiveAgent { thread_id, metadata, status }
```

注意第 3/4/5 步是**三种不同的限额，各管各的**：

| 限额 | 结构 | 管什么 | 默认值 |
|---|---|---|---|
| 并发活跃度 | `AgentExecutionLimiter` | 同时**正在跑 turn** 的代理数 | V2: 4 |
| 驻留 | `V2Residency` | 同时**加载在内存**的子线程数 | 同上（capacity 复用） |
| 总数 | `AgentRegistry.total_count` | 会话树内**注册过**的代理数 | V1: 6 |

V2 走驻留路径时会把总数限额设成 `None`（`spawn.rs:396`），因为驻留 LRU 已经在管了，两套一起卡会互相打架。

### 4.3 Fork 的历史裁剪

`keep_forked_rollout_item`（`spawn.rs:47`）决定复制哪些条目：

```rust
match item {
    // 保留
    ResponseItem::Message { role: "system" | "developer" | "user", .. } => true,
    ResponseItem::Message { role: "assistant", phase } =>
        *phase == Some(MessagePhase::FinalAnswer),      // 只留最终回答
    RolloutItem::Compacted(_) | EventMsg(_) | SessionMeta(_) => true,

    // 丢弃
    Reasoning | FunctionCall | FunctionCallOutput | LocalShellCall
    | ToolSearchCall | WebSearchCall | ImageGenerationCall
    | Compaction | ContextCompaction | AdditionalTools | AgentMessage => false,
    InterAgentCommunication(_) | InterAgentCommunicationMetadata { .. } => false,

    // 条件保留
    TurnContext(_) | WorldState(_) => preserve_reference_context_item,
}
```

也就是说 fork 出来的子代理看到的是**父代理的对话骨架**：用户说了什么、系统指令是什么、助手给过哪些最终结论。中间的工具调用、推理过程、其他代理的消息全被撕掉。

理由是显然的——子代理不需要知道父代理是怎么 grep 出来的，只需要知道结论。这同时大幅压缩了 fork 的 token 成本。

最后那条 `TurnContext | WorldState` 的注释解释了为什么要区分：

> 全量 fork 保留了缓存的 prompt 前缀，可以继续从父代理的持久基线做增量。截断 fork 丢掉了那个前缀的一部分，所以必须在第一个子 turn 上重建上下文。

`fork_turns = N` 的按 turn 边界裁剪在 `thread_rollout_truncation.rs`，靠扫描 `ResponseItem::Message` 并用 `event_mapping::parse_turn_item` 判断是不是 `TurnItem::UserMessage` 来定位边界。它还处理 `ThreadRolledBack` 标记（用户回退过的 turn 不算数）。

---

## 5. 通信：mailbox 体系

### 5.1 消息类型

`protocol/src/protocol.rs:740`

```rust
pub struct InterAgentCommunication {
    pub id: Option<ResponseItemId>,
    pub author: AgentPath,               // 发件人
    pub recipient: AgentPath,            // 主收件人
    pub other_recipients: Vec<AgentPath>,// 抄送
    pub content: String,
    pub encrypted_content: Option<String>,
    pub internal_chat_message_metadata_passthrough: Option<...>,
    pub trigger_turn: bool,              // ← 关键标志
}
```

`trigger_turn` 是整个通信语义的分水岭：

- `true`：投递并唤醒目标开始新 turn（`spawn` 的初始任务、`followup_task`）
- `false`：只入队，等目标下次自然开 turn 时一起消费（`send_message`、子代理完成回传）

### 5.2 三种信封格式

模型看到的不是裸文本，而是三种结构化信封。

**MESSAGE / NEW_TASK**（`context/inter_agent_message.rs`，role = `assistant`）：

```
Message Type: NEW_TASK
Task name: /root/task1/task_3
Sender: /root/task1
Payload:
<实际内容>
```

**FINAL_ANSWER**（`context/inter_agent_completion_message.rs:37`，子代理完成时回传）：

```
Message Type: FINAL_ANSWER
Task name: /root/task1
Sender: /root/task1/task_3
Payload:
<子代理的最终回答>
```

**subagent_notification**（`context/subagent_notification.rs`，仅 V1，role = `user`）：

```xml
<subagent_notification>
{"agent_path":"task_3","status":{"completed":"..."}}
</subagent_notification>
```

V1 和 V2 的差别在这里体现得最直观：**V1 把完成通知伪装成用户消息塞进父上下文，V2 用统一的 assistant-role 信封走 mailbox**。V1 的做法会让模型看到一条自己没说过的"用户消息"，语义是脏的。

对应的系统提示里明确教了模型怎么读这个信封（`config/mod.rs:213` 的 root usage hint）：

```
You will receive messages in the analysis channel in the form:
Message Type: MESSAGE | FINAL_ANSWER
Task name: <recipient>
Sender: <author>
Payload:
<payload text>
```

### 5.3 mailbox 实现

`core/src/session/input_queue.rs:35`

```rust
pub(crate) struct InputQueue {
    activity_tx: watch::Sender<InputQueueActivity>,   // Mailbox | Steer
    mailbox_pending_mails: Mutex<VecDeque<PendingMailboxCommunication>>,
}
```

只有两个操作面：

```rust
// 投递（写侧）
enqueue_mailbox_communication(communication, parent_turn_id) {
    push_back(...);
    activity_tx.send_replace(InputQueueActivity::Mailbox);   // ← 唤醒所有 waiter
}

// 排空（读侧，turn 开始时）
drain_mailbox_input_items() -> (Vec<TurnInput>, Option<String>)
```

`drain` 里有一段细节（`input_queue.rs:111`）：多封信里如果 `trigger_turn` 的那些来自**同一个** parent_turn_id 才保留，否则丢弃。用 `reduce` 做的：

```rust
.map(|mail| mail.parent_turn_id.as_deref())
.reduce(|expected, candidate| expected.filter(|id| candidate == Some(*id)))
```

这是为了 trace 归因——一批信来自不同父 turn 时无法归到单个 turn 上。

还有个 `MailboxDeliveryPhase` 状态（`defer_mailbox_delivery_to_next_turn` / `accept_mailbox_delivery_for_current_turn`），控制"当前 turn 是否接受插入新邮件"。某些 turn 类型（review、compact）不能被打断。

### 5.4 wait_agent：两代的语义分裂

**V1**（`handlers/multi_agents/wait.rs`，324 行）：

```rust
struct WaitArgs {
    targets: Vec<String>,      // 必填：等哪些 agent
    timeout_ms: Option<i64>,
}
```

对每个 target 拿一个 `watch::Receiver<AgentStatus>`，塞进 `FuturesUnordered`，谁先到终态谁先返回。返回值里带**完整的最终消息**。

**V2**（`handlers/multi_agents_v2/wait.rs`，196 行）：

```rust
struct WaitArgs {
    timeout_ms: Option<i64>,   // 就这一个参数
}

pub(crate) struct WaitAgentResult {
    message: String,     // "Wait completed." / "Wait interrupted by new input." / "Wait timed out."
    timed_out: bool,
}
```

只等**本会话 mailbox 有任何动静**：

```rust
async fn wait_for_activity(activity_rx, pending_activity, deadline) -> WaitOutcome {
    if let Some(activity) = pending_activity { return ...; }   // 已经有存量，立刻返回
    match timeout_at(deadline, activity_rx.changed()).await {
        Ok(Ok(())) => match *activity_rx.borrow_and_update() {
            InputQueueActivity::Mailbox => WaitOutcome::MailboxActivity,
            InputQueueActivity::Steer   => WaitOutcome::Steered,   // 用户插话也算
        },
        _ => WaitOutcome::TimedOut,
    }
}
```

**V2 的 wait 不返回任何内容**，因为内容已经通过 mailbox 进了上下文。这个设计消除了三类问题：

1. 不会"等 A 的时候 B 完成了却唤不醒"
2. 不会出现内容重复（wait 返回一份 + mailbox 一份）
3. 用户插话（Steer）也能中断等待，不会卡住

超时范围（`config/mod.rs:210`）：min 10s / default 30s / max 1 小时。工具描述里明写"**优先用长等待（分钟级）以避免忙轮询**"。min 10s 的存在就是硬性防忙轮询——`multi_agents_common.rs:29` 的注释："Minimum wait timeout to prevent tight polling loops from burning CPU."

### 5.5 完成回传：两条不同的路径

**V1**：`AgentControl::maybe_start_completion_watcher`（`control.rs:459`）起一个 detached tokio task：

```rust
tokio::spawn(async move {
    // 订阅子的 status watch，循环到终态
    while !is_final(&status) { status_rx.changed().await?; status = ...; }
    // 格式化成 <subagent_notification> 并注入父上下文
    parent_thread.inject_user_message_without_turn(message).await;
});
```

**V2**：不起 watcher（`spawn.rs:530` 有 `if multi_agent_version != V2` 的守卫）。改由**子代理自己在会话内**完成回传（`session/mod.rs:1918` `forward_child_completion_to_parent`）：

```rust
// 子代理的事件处理里，检测到自己进入终态
let Some(message) = format_inter_agent_completion_message(parent_path, child_path, &status);
let communication = InterAgentCommunication::new(
    child_agent_path, parent_agent_path, vec![], message,
    /*trigger_turn*/ false,       // ← 不唤醒父代理
);
agent_control.send_inter_agent_communication(parent_thread_id, communication, ctx, None).await;
```

V2 的改进：
- 不需要额外的后台 task（一个子代理省一个 tokio task）
- 回传发生在子代理自己的执行上下文里，能顺带写 rollout trace
- `trigger_turn = false`——父代理**不会被打断**，等它自己 wait 或下次开 turn 时消费

`format_inter_agent_completion_message`（`session_prefix.rs:27`）的状态映射：

| AgentStatus | 回传 payload |
|---|---|
| `Completed(Some(msg))` | msg 原文 |
| `Completed(None)` | 空串 |
| `Errored(e)` | 截断到 900 token 的错误 + "这个 agent 的 turn 失败了。如果还需要它，用协作工具再给它一个任务。" |
| `Shutdown` | "Agent shut down." |
| `NotFound` | "Agent was not found." |
| `PendingInit`/`Running`/`Interrupted` | `None`（不回传） |

完成消息总预算 1000 token，其中 100 token 留给信封（`session_prefix.rs:10-13`）。

---

## 6. 角色系统

### 6.1 角色即 config layer

`core/src/agent/role.rs` 的模块注释一句话说清了设计：

> 角色在 spawn 时选定，用和 `config.toml` 相同的配置机制加载。这个模块解析内置和用户定义的角色文件，把角色作为一个高优先级层插入，并保留调用者当前的 model、reasoning effort、provider、service tier——除非角色层自己设置了它们。**它不决定何时 spawn 或用哪个角色；那是多代理工具 handler 的职责。**

关键实现（`role.rs:254`）：

```rust
fn role_layer(role_layer_toml: TomlValue) -> ConfigLayerEntry {
    ConfigLayerEntry::new(ConfigLayerSource::SessionFlags, role_layer_toml)
}
```

角色被插到 `SessionFlags` 优先级——能压过持久化的 config.toml，但仍在命令行参数之下。然后整个 config 用 `Config::load_config_with_layer_stack` **完整重建一遍**，而不是打补丁。

"粘性运行时选择"的处理很细（`role.rs:108`）：

```rust
let preserve_current_provider = role_layer_toml.get("model_provider").is_none();
let preserve_current_service_tier = role_layer_toml.get("service_tier").is_none();
let preserve_current_model = role_layer_toml.get("model").is_none();
let preserve_current_reasoning_effort = role_layer_toml.get("model_reasoning_effort").is_none();
```

即：**角色文件没写的字段，保持父代理当前的运行时选择，而不是回落到默认值**。注释直说了为什么：

> 不带这些覆盖重建 config 会让 spawn 出的代理静默回落到默认设置。

### 6.2 三个内置角色

`role.rs:363-428`，全部 hardcode 在 `built_in::configs()` 里。

| 角色 | config_file | 描述要点 |
|---|---|---|
| `default` | 无 | "Default agent." |
| `explorer` | `explorer.toml`（**0 字节，空文件**） | 快速权威的代码库问答 |
| `worker` | 无 | 执行生产性工作 |
| ~~`awaiter`~~ | `awaiter.toml` | 已被注释掉（"temp removed"） |

`explorer.toml` 是空文件——意味着 explorer **没有任何配置覆盖，全靠描述文本引导模型**。真正起作用的是那段描述：

> Explorer 快速且权威。必须用来问代码库上具体、范围明确的问题。
> - 为避免重复劳动，你应该避免探索 explorer 已经覆盖过的问题。通常你应该信任 explorer 的结果，不做额外验证。
> - **鼓励你在有多个可独立回答的不同问题时并行 spawn 多个 explorer。** 等结果时你可以继续做不依赖这些结果的本地工作。这种并行性是委派的关键优势。
> - 相关问题复用已有的 explorer。

`worker` 的描述里有两条工程经验特别值得抄：

> - **明确分配任务的 ownership（文件/职责）**。涉及代码改动时明确指出 worker 负责哪些文件或模块。例如"Worker 1 负责更新认证模块，Worker 2 处理数据库层"。
> - **永远告诉 worker 他们不是代码库里唯一的人**，不要回滚别人的修改，要调整自己的实现以适配别人的改动。

被注释掉的 `awaiter.toml` 内容也很有意思——它是一个"只负责等待长任务完成并报告状态"的角色，配置里设了 `background_terminal_max_timeout = 3600000`、`model_reasoning_effort = "low"`，developer_instructions 是一份严格的行为守则（不许修改任务、不许优化、不许幻觉完成、每次重试指数增加超时）。**用低成本模型专职做轮询等待**，是个聪明的成本优化，可惜暂时下线了。

### 6.3 角色描述如何进入模型视野

`role.rs:278` 的 `spawn_tool_spec::build`：把所有角色（用户定义优先于内置）格式化成一段文本，拼进 `agent_type` 参数的 schema 描述。还会读角色文件，如果里面锁死了 model / reasoning_effort / service_tier，就追加一句：

> This role's model is set to `xxx` and its reasoning effort is set to `yyy`. These settings cannot be changed.

**让模型知道哪些参数它改不了**，避免它徒劳地传覆盖参数。

用户自定义角色走 config 的 `agent_roles` 表，从 `AgentRoleConfig { description, config_file, nickname_candidates }` 加载。`nickname_candidates` 允许角色自带昵称池（比如给 explorer 一批探险家名字）。

---

## 7. 触发策略：模型什么时候能派工

这一块是纯 prompt engineering，代码只负责选文本。

### 7.1 MultiAgentMode 决策

`core/src/session/multi_agents.rs:39`

```rust
pub(crate) fn effective_multi_agent_mode(turn_context: &TurnContext) -> Option<MultiAgentMode> {
    if turn_context.multi_agent_version != MultiAgentVersion::V2 { return None; }

    let mode = match &turn_context.config.multi_agent_v2.multi_agent_mode_hint_text {
        Some(hint_text) => MultiAgentMode::Custom(hint_text.clone()),
        None => match turn_context.effective_reasoning_effort() {
            Some(ReasoningEffort::Ultra) => MultiAgentMode::Proactive,   // ← 唯一自动路径
            _ => MultiAgentMode::ExplicitRequestOnly,
        },
    };
    // Internal 来源（review/compact）返回 None，不参与多代理
    match &turn_context.session_source { ... }
}
```

只有一个自动开关：**reasoning effort 拉到 Ultra 才进主动模式**。其余都要用户显式要求。

### 7.2 工具描述里的硬性规定

`multi_agents_spec.rs:714`（V1 spawn 的描述）：

> Do not spawn sub-agents unless the user or applicable AGENTS.md/skill instructions explicitly ask for sub-agents, delegation, or parallel agent work.
> Requests for depth, thoroughness, research, investigation, or detailed codebase analysis **do not count** as permission to spawn.
> Agent-role guidance below only helps choose which agent to use after spawning is already authorized; it never authorizes spawning by itself.

第二句是重点——**"帮我深入调研一下"不算授权**。第三句更细：角色描述里那些"鼓励并行 spawn 多个 explorer"的话，只在已经被授权后才生效，本身不构成授权。这是防止 prompt 之间互相"越权解释"。

### 7.3 委派方法论（也在工具描述里）

V1 spawn 的描述包含四个小节的完整方法论，值得整段抄：

**何时委派 vs 自己做**
- 先快速分析总任务，形成简洁的高层计划。识别哪些是关键路径上的**即时阻塞项**，哪些是需要但可以并行跑的**旁挂任务**。作为计划的一部分，明确决定你现在应该在本地做哪个即时任务。**在委派之前先做这步规划，免得把即时阻塞任务交出去然后干等。**
- 子任务足够简单且能和你的本地工作并行时才用 subagent。
- **不要委派紧急的阻塞工作**。如果下一步动作就依赖这个结果，主 rollout 通常应该自己做，保持关键路径推进。
- 子任务太难、耦合太紧、太紧急或很可能阻塞下一步时，留在本地。

**怎么设计委派的子任务**
- 必须具体、定义清晰、自包含。
- 必须实质性推进主任务。
- 主 rollout 和委派任务之间不要重复劳动。
- 同一个未解决的线程上不要发多个委派调用，除非新任务确实不同且必要。
- **代码任务优先委派具体的代码改动 worker，而不是只读的 explorer 分析**——只要 subagent 能在明确的 write scope 内做一个有边界的补丁。
- 委派代码工作时，指示子模型直接在它的 forked workspace 里改文件，并在最终回答里列出改过的文件路径。
- **代码编辑子任务要分解到每个任务的 write set 不相交。**

**委派之后**
- **`wait_agent` 要非常克制地调用。** 只有当你需要结果做下一个关键路径步骤、且在它返回前完全被阻塞时才调。
- 不要自己重做已委派的任务；专注于整合结果或处理不重叠的工作。
- subagent 在后台跑的时候，立刻做有意义的不重叠工作。
- **不要反射性地反复等待。**

**并行委派模式**
- 有多个可独立回答的不同问题时，并行跑多个信息搜集子任务。
- write scope 不重叠时，把实现拆成不相交的代码切片并行 spawn。
- 验证工作只在能和实现并行、且很可能在最终整合前抓到具体风险时才委派。

### 7.4 共享文件系统的告知

`config/mod.rs:254` 的 `DEFAULT_MULTI_AGENT_V2_SHARED_USAGE_HINT_TEXT`：

> All agents share the same directory. In detail:
> - All agents have access to the same container and filesystem as you.
> - All agents use the same current working directory.
> - **As a result, edits made by one agent are immediately visible to all other agents.**

以及并发槽数量会被拼进去：

> There are {max_concurrency} available concurrency slots, meaning that up to {max_concurrency} agents can be active at once, including you.

**把限额数字告诉模型**，让它自己控制 spawn 节奏，而不是等着报 `AgentLimitReached`。

---

## 8. 工具面

### 8.1 注册门控

`core/src/tools/spec_plan.rs:886` `add_collaboration_tools`：

```rust
fn collab_tools_enabled(turn_context: &TurnContext) -> bool {
    match turn_context.multi_agent_version {
        MultiAgentVersion::Disabled => false,
        MultiAgentVersion::V1 => !exceeds_thread_spawn_depth_limit(
            next_thread_spawn_depth(&turn_context.session_source),
            turn_context.config.agent_max_depth,       // 默认 1
        ),
        MultiAgentVersion::V2 => true,                 // ← V2 无深度限制
    }
}
```

**V1 用"不注册工具"来实现深度限制**——超过深度的代理根本看不到 `spawn_agent`，不是调用后报错。这比运行时拒绝干净：模型不会浪费一次调用，也不会因为工具存在而反复尝试。

V2 取消了深度限制，允许任意层级树状分包。

### 8.2 两代工具对照

| | V1 | V2 |
|---|---|---|
| Namespace | `multi_agent_v1` | `collaboration`（可配） |
| 暴露方式 | 有 ToolSearch 时 `Deferred`，否则 `Direct` | `Direct` 或 `DirectModelOnly` |
| 寻址 | ThreadId 字符串 | AgentPath（相对或绝对） |
| spawn | `{ message?, items?, agent_type?, model?, reasoning_effort?, service_tier?, fork_context: bool }` | `{ message*, task_name*, agent_type?, model?, reasoning_effort?, service_tier?, fork_turns? }` |
| spawn 返回 | `{ agent_id, nickname }` | `{ task_name }`（默认隐藏元数据）或 `{ task_name, nickname }` |
| 发消息 | `send_input { target, message?/items?, interrupt? }` | `send_message { target, message }`（不起 turn）<br>`followup_task { target, message }`（起 turn） |
| 等待 | `wait_agent { targets*, timeout_ms? }` → 带最终消息 | `wait_agent { timeout_ms? }` → 只带摘要 |
| 中断 | 无独立工具（`send_input` 的 `interrupt=true`） | `interrupt_agent` |
| 列表 | 无 | `list_agents` |
| 关闭/恢复 | `close_agent` / `resume_agent` | 无（由 residency 自动管理） |

V1 的 `send_input` 描述里有一句设计意图：

> 如果你认为分配的任务高度依赖某个之前任务的上下文，你应该用 send_input 复用那个 agent。

V2 拆成 `send_message` / `followup_task` 是因为"投消息"和"派新活"是两件事：前者是补充信息，后者要唤醒目标。V1 用一个工具加 `interrupt` 布尔值表达，语义混在一起。

### 8.3 handler 的统一形状

每个 handler 都实现同一组 trait：

```rust
impl ToolExecutor<ToolInvocation> for Handler {
    fn tool_name(&self) -> ToolName;                    // plain 或 namespaced
    fn spec(&self) -> ToolSpec;                         // 从 multi_agents_spec.rs 取
    fn search_info(&self) -> Option<ToolSearchInfo>;    // 仅 V1：ToolSearch 关键词
    fn handle(&self, invocation) -> ToolExecutorFuture;
}
impl CoreToolRuntime for Handler {
    fn matches_kind(&self, payload: &ToolPayload) -> bool;
}
```

输出统一实现 `ToolOutput`（`log_preview` / `success_for_logging` / `to_response_item` / `code_mode_result`），序列化用 `multi_agents_common.rs` 里的四个泛型 helper 复用。

错误映射也集中在 common（`multi_agents_common.rs:84-109`）：

```rust
collab_spawn_error(err)   // "thread manager dropped" → "collab manager unavailable"
collab_agent_error(id, err) {
    ThreadNotFound(id)     → "agent with id {id} not found"
    InternalAgentDied      → "agent with id {id} is closed"
    UnsupportedOperation   → "collab manager unavailable"
}
```

全部返回 `FunctionCallError::RespondToModel`——**多代理的错误从不中断 turn，只是给模型一条工具结果让它自己决策**。

### 8.4 TurnItem 事件

每次协作工具调用都发两个事件（started + completed），载体是：

```rust
CollabAgentToolCallItem {
    id: call_id,
    tool: CollabAgentTool::{ Spawn | Wait | CloseAgent | ... },
    status: InProgress | Completed | Failed,
    sender_thread_id: ThreadId,
    receiver_thread_ids: Vec<ThreadId>,
    receiver_agents: Vec<CollabAgentRef>,   // { thread_id, nickname, role }
    prompt: Option<String>,
    model / reasoning_effort: Option<...>,
    agents_states: HashMap<ThreadId, CollabAgentState>,
}
```

V2 额外有 `SubAgentActivityItem { id, agent_thread_id, agent_path, kind: Started | ... }`，由 `emit_sub_agent_activity`（`multi_agents_v2.rs:47`）同时发 started 和 completed（因为 spawn 是瞬时的）。

---

## 9. 资源约束的四层

### 9.1 并发活跃度：AgentExecutionLimiter

`core/src/agent/control/execution.rs:15`

```rust
pub(super) struct AgentExecutionLimiter {
    active: AtomicUsize,
    max_threads: OnceLock<usize>,     // 会话创建时一次性初始化
}
```

只在**开始新 turn**时检查（`execution.rs:107`）：

```rust
fn op_starts_turn(op: &Op) -> bool {
    matches!(op, Op::UserInput { .. })
        || matches!(op, Op::InterAgentCommunication { communication }
                        if communication.trigger_turn)
}
```

只对 **V2 且是子代理**生效（`execution.rs:112`）——根代理不占槽，或者说根代理是"包含在 max_concurrency 里的那个 1"，这也是为什么 `effective_agent_max_threads` 对 V2 返回 `max_concurrent_threads_per_session - 1`（`config/mod.rs:1554`）。

已经在跑 turn 的线程不重复检查（`execution.rs:50`）：

```rust
if thread.session.active_turn.lock().await.is_some() { return Ok(()); }
```

`AgentExecutionGuard` 同样是 RAII，Drop 时 `fetch_sub`。

### 9.2 驻留：V2Residency

`core/src/agent/control/residency.rs:18`

```rust
struct V2ResidencyState {
    residents: VecDeque<ThreadId>,   // LRU：front 最旧，back 最新
    pending_slots: usize,            // 正在 spawn 中、还没 commit 的名额
}
```

预留逻辑是个循环（`residency.rs:81`）：

```rust
loop {
    if try_reserve_pending_slot(capacity) { return Ok(slot); }   // 有空位
    if !try_unload_one_resident(manager, protected).await {      // 没空位，尝试卸载
        return Err(AgentLimitReached { max_threads: capacity });
    }
}
```

卸载条件（`residency.rs:226`）三个都满足才行：

```rust
async fn is_unloadable(thread: &CodexThread) -> bool {
    matches!(thread.agent_status().await,
             Completed(_) | Errored(_) | Interrupted)
        && thread.session.active_turn.lock().await.is_none()
        && !thread.session.input_queue.has_pending_mailbox_items().await
}
```

**有未读邮件的代理不能被卸载**——否则消息就丢了。

卸载流程严格保序（`residency.rs:139`）：

```rust
candidate_thread.ensure_rollout_materialized().await;   // 1. 先落盘
candidate_thread.shutdown_and_wait().await?;            // 2. 再优雅关闭
manager.remove_thread(&candidate_thread_id).await;      // 3. 最后摘表
```

任何一步失败就 `touch` 回队尾重新排队，不硬删。

`protected_thread_id` 参数保护正在操作的线程不被自己的 spawn 卸掉（`residency.rs:169`）。

### 9.3 总数：AgentRegistry.total_count

见 §2.4。V1 用（默认 6），V2 走 residency 时不用。

### 9.4 Token 预算：RolloutBudget

`core/src/rollout_budget.rs:17`

```rust
/// Shared accounting and reminder state for one root-thread session tree.
pub(crate) struct RolloutBudget {
    state: OnceLock<Mutex<RolloutBudgetState>>,
}

struct RolloutBudgetState {
    config: RolloutBudgetConfig,
    weighted_tokens_used: f64,
    deliveries: HashMap<ThreadId, ThreadBudgetDelivery>,  // 每线程的提醒进度
}
```

加权记账（`rollout_budget.rs:43`）：

```rust
weighted_tokens_used += output_tokens * sampling_token_weight
                      + non_cached_input() * prefill_token_weight;
```

**输出 token 和未命中缓存的输入 token 权重不同**——反映真实成本差异。缓存命中的输入不计费。

超预算时 turn 被标记为 `BudgetLimited`，在 `status.rs:14` 映射成 `AgentStatus::Interrupted`：

```rust
EventMsg::TurnAborted(ev) => match ev.reason {
    Interrupted | BudgetLimited => Some(AgentStatus::Interrupted),
    _ => Some(AgentStatus::Errored(...)),
}
```

`Interrupted` 不是终态（`is_final` 返回 false），所以**不会触发完成回传**——代理还能接收更多输入。

提醒机制按线程独立追踪（`pending_reminder` / `mark_reminder_delivered`），保证每个线程都能观察到跨过的阈值，不会因为兄弟已经收到过就跳过。`window_id` 参与判定是为了压缩后重新提醒。

---

## 10. 展示层

### 10.1 TUI

`tui/src/multi_agents.rs`（970 行）渲染成历史行：

```
• Spawned Robie [explorer]
  Waiting for 2 agents
  Completed - 认证模块用的是 JWT，token 在…
```

关键函数：

| 函数 | 输出 |
|---|---|
| `spawn_end` | `Spawned <nickname> [role]` |
| `waiting_begin` | `Waiting for N agents`（0 个时 `Waiting for agents`） |
| `waiting_end` / `wait_complete_lines` | 每个 agent 的状态摘要行 |
| `close_end` / `resume_begin` / `resume_end` | 关闭/恢复 |
| `sub_agent_activity_history_cell` | V2 的 SubAgentActivity 渲染 |
| `agent_picker_status_dot_spans` | `/agent` 选择器里的状态点 |
| `previous_agent_shortcut` / `next_agent_shortcut` | Alt+←/→ 切换 |

`/agent`（别名 `/subagents`）打开选择器，切进任意子线程看完整 transcript。

### 10.2 app-server

`thread/list` 支持 `parentThreadId` 过滤，thread 记录带父子关系字段。`notify_thread_created`（spawn 流程第 12 步）让客户端能及时订阅新线程的事件流。

### 10.3 可观测性

**OTel 通信日志**（`agent_communication.rs`）：target `codex_otel.agent_communication`，每条消息发送和接收各一条：

```rust
tracing::info!(target: AGENT_COMMUNICATION_TARGET, {
    event.name = "codex.agent_communication",
    communication_id, kind, state = "send",
    sender_thread_id, receiver_thread_id,
    content = communication.encrypted_content.as_deref().unwrap_or("[plaintext]"),
}, "agent communication");
```

`kind` 有四种：`spawn | message | followup | result`。注意日志只记加密内容或 `[plaintext]` 占位符，不泄漏明文。

**指标**：
- `codex.multi_agent.spawn { role, version }`（spawn.rs:158）
- `codex.multi_agent.nickname_pool_reset`（registry.rs:227）

---

## 11. V1 vs V2 的代码级差异

| 维度 | V1 | V2 | 差异所在文件 |
|---|---|---|---|
| 默认开关 | 默认开启 | 默认关闭 | features |
| 身份 | ThreadId + 随机昵称 | AgentPath 层级路径 | agent_path.rs |
| 深度限制 | `agent_max_depth = 1`，靠不注册工具实现 | 无限制 | spec_plan.rs:402 |
| 并发口径 | 总线程数 ≤ 6 | 同时活跃 turn ≤ 4 | execution.rs vs registry.rs |
| 空闲处理 | `close_agent` 手动关，`resume_agent` 手动开 | residency LRU 自动卸载/加载 | residency.rs |
| 完成回传 | detached watcher → 伪装成 user 消息 | 子代理自己在会话内发 → assistant 信封 | control.rs:459 vs session/mod.rs:1918 |
| 回传信封 | `<subagent_notification>` + JSON | `Message Type: FINAL_ANSWER` | subagent_notification.rs vs inter_agent_completion_message.rs |
| wait 语义 | 等指定 targets，返回全文 | 等 mailbox 活动，只返回摘要 | multi_agents/wait.rs vs multi_agents_v2/wait.rs |
| 追加任务 | `send_input`（含 interrupt 标志） | `send_message` + `followup_task` 分开 | handlers 目录 |
| 通信范围 | 基本父↔子 | 任意 agent 按路径互发 | resolve_agent_reference |
| fork 粒度 | `fork_context: bool` | `fork_turns: "none"/"all"/"N"` | spawn.rs fork_mode() |
| 子代理人设 | 与父共用 developer instructions | `subagent_developer_instructions` 单独替换 | role.rs:54 |
| 主动性 | 只有显式请求 | Ultra effort → Proactive | session/multi_agents.rs:39 |
| 用户可见元数据 | 返回 agent_id + nickname | 默认只返回 task_name | `hide_spawn_agent_metadata: true` |

两代共用的部分：`multi_agents_spec.rs`（Schema 定义）、`multi_agents_common.rs`（参数解析/错误/config 构建）、`AgentControl`、`AgentRegistry`、`role.rs`。这说明团队是把 V2 当 V1 的**直接继任者**在维护，不是两套并行实现。

---

## 12. 设计哲学总结

把散落的决策归纳成几条原则：

**1. 子代理不是新概念，是既有概念的复用。**
`CodexThread` 本来就存在（会话恢复、review、compact 都用它）。多代理只是给它加了 `SessionSource::SubAgent(ThreadSpawn{...})` 这一种新来源。运行时代码零改动，这是整个设计能维持复杂度的根本原因。

**2. 身份先于一切。**
`AgentPath` 定义好之后，寻址、拓扑、日志、UI 显示、持久化全都水到渠成。V1 用 ThreadId 时这些都得各自处理。

**3. 所有资源约束用 RAII。**
名额、驻留槽、执行槽三处都是 `commit()` + `Drop` 回滚。spawn 流程有十几个提前返回点，没有任何一处写清理代码。

**4. 异步 + mailbox，不做阻塞式子任务。**
`spawn_agent` 立刻返回，结果通过 mailbox 异步到达。`wait_agent` 等"任何动静"而不是"某个 agent"。这消除了死锁，也让并行成为默认而非特例。

**5. 策略写在 prompt 里，不写在代码里。**
"什么时候该派工""怎么划分 write set""不要反射性等待"——这些全在工具描述文本里。代码只负责选哪段文本。好处是调整策略不用改代码，坏处是 890 行的 spec 文件。

**6. 限额数字告诉模型。**
并发槽数拼进 usage hint，角色的锁定参数拼进 schema 描述。让模型有信息自我约束，而不是撞墙后重试。

**7. 共享文件系统是刻意的，冲突靠纪律防。**
多代理的价值就是并行改同一个仓库，隔离了就失去意义。代价是没有技术手段防冲突，只能靠"划分不相交 write set"的提示词纪律。这是明确接受的取舍。

**8. 错误从不中断 turn。**
所有多代理错误都是 `RespondToModel`，变成一条工具结果让模型自己决策。子代理失败的回传消息里甚至附带下一步建议（"如果还需要它，用协作工具再给它一个任务"）。

---

## 附：完整文件索引

### protocol
| 文件 | 内容 |
|---|---|
| `protocol/src/agent_path.rs` | `AgentPath`（185 行含测试） |
| `protocol/src/protocol.rs:740` | `InterAgentCommunication` |
| `protocol/src/protocol.rs:1736` | `AgentStatus` |
| `protocol/src/protocol.rs:2843` | `SubAgentSource` |
| `protocol/src/protocol.rs:3044` | `MultiAgentVersion` |
| `protocol/src/config_types.rs:325` | `MultiAgentMode` |

### agent-graph-store（独立 crate）
| 文件 | 行数 | 内容 |
|---|---:|---|
| `src/store.rs` | 60 | `AgentGraphStore` trait |
| `src/types.rs` | 42 | `ThreadSpawnEdgeStatus` |
| `src/local.rs` | 344 | SQLite 实现 |
| `src/error.rs` | 20 | 错误类型 |

### core/src/agent
| 文件 | 行数 | 内容 |
|---|---:|---|
| `control.rs` | 804 | `AgentControl` 主体 |
| `control/spawn.rs` | 1013 | spawn 全流程 + fork |
| `control/execution.rs` | 122 | 并发活跃度限额 |
| `control/residency.rs` | 236 | LRU 驻留 |
| `control/legacy.rs` | 103 | V1 兼容 |
| `registry.rs` | 347 | 注册表 + 预留 |
| `role.rs` | 444 | 角色 config layer |
| `agent_resolver.rs` | 37 | target 解析 |
| `status.rs` | 28 | 事件→状态映射 |
| `builtins/explorer.toml` | 0 | 空（explorer 无配置覆盖） |
| `builtins/awaiter.toml` | 38 | 已下线角色 |
| `agent_names.txt` | — | 昵称池 |
| `control_tests.rs` | 4236 | 测试 |
| `registry_tests.rs` | 573 | 测试 |
| `role_tests.rs` | 556 | 测试 |

### core/src 其他
| 文件 | 内容 |
|---|---|
| `agent_communication.rs` | OTel 埋点 + `AgentCommunicationKind` |
| `session_prefix.rs` | 三种信封的格式化入口 |
| `context/inter_agent_message.rs` | MESSAGE / NEW_TASK |
| `context/inter_agent_completion_message.rs` | FINAL_ANSWER |
| `context/subagent_notification.rs` | V1 通知信封 |
| `session/input_queue.rs` | mailbox |
| `session/multi_agents.rs` | MultiAgentMode 决策 |
| `session/mod.rs:1918` | V2 完成回传 |
| `rollout_budget.rs` | 树级 token 预算 |
| `thread_rollout_truncation.rs` | turn 边界裁剪 |
| `codex_thread.rs` | `CodexThread` 定义 |
| `thread_manager.rs` | 线程注册与生命周期（2165 行） |

### core/src/tools
| 文件 | 行数 | 内容 |
|---|---:|---|
| `spec_plan.rs:886` | — | `add_collaboration_tools` |
| `handlers/multi_agents_spec.rs` | 890 | 所有 Schema + 描述文本 |
| `handlers/multi_agents_common.rs` | 463 | 共用工具函数 |
| `handlers/multi_agents.rs` | 99 | V1 模块入口 |
| `handlers/multi_agents/spawn.rs` | 261 | V1 spawn |
| `handlers/multi_agents/wait.rs` | 324 | V1 wait |
| `handlers/multi_agents/send_input.rs` | 158 | V1 发消息 |
| `handlers/multi_agents/close_agent.rs` | 164 | V1 关闭 |
| `handlers/multi_agents/resume_agent.rs` | 209 | V1 恢复 |
| `handlers/multi_agents_v2.rs` | 84 | V2 模块入口 |
| `handlers/multi_agents_v2/spawn.rs` | 260 | V2 spawn |
| `handlers/multi_agents_v2/wait.rs` | 196 | V2 wait |
| `handlers/multi_agents_v2/message_tool.rs` | 130 | 消息共用 |
| `handlers/multi_agents_v2/send_message.rs` | 46 | 投消息 |
| `handlers/multi_agents_v2/followup_task.rs` | 46 | 派新活 |
| `handlers/multi_agents_v2/interrupt_agent.rs` | 131 | 中断 |
| `handlers/multi_agents_v2/list_agents.rs` | 83 | 列表 |
| `handlers/multi_agents_tests.rs` | 4538 | 测试 |

### 配置默认值（core/src/config/mod.rs）
| 常量 | 值 | 行 |
|---|---|---|
| `DEFAULT_AGENT_MAX_THREADS` | `Some(6)` | 208 |
| `DEFAULT_MULTI_AGENT_V2_MAX_CONCURRENT_THREADS_PER_SESSION` | 4 | 209 |
| `DEFAULT_MULTI_AGENT_V2_MIN_WAIT_TIMEOUT_MS` | 10_000 | 210 |
| `DEFAULT_MULTI_AGENT_V2_MAX_WAIT_TIMEOUT_MS` | 3_600_000 | 211 |
| `DEFAULT_MULTI_AGENT_V2_DEFAULT_WAIT_TIMEOUT_MS` | 30_000 | 212 |
| `DEFAULT_MULTI_AGENT_V2_TOOL_NAMESPACE` | `"collaboration"` | 253 |
| `DEFAULT_AGENT_MAX_DEPTH` | 1 | 270 |
| `hide_spawn_agent_metadata` | true | 1279 |
| `wait_agent_enabled` | true | 1281 |
| `non_code_mode_only` | true | 1282 |
