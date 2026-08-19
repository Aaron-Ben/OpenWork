# 协作模式

一个常驻 daemon 驱动一个本地 OpenCode 服务，把若干个各有私有 home 的 session 变成**对等同事**，它们在共享房间里协作，通过 daemon 提供的 MCP 工具对世界做动作。

协作模式与现有单会话工作台在**进程、数据、引擎、文件系统**四层不相交。它不是"把子 Agent 放开成可写"——[multi-agent.md](multi-agent.md) 的子 Agent 是任务级从属、只读、单层、由 Tool Call 派生；这里的 Agent 是身份级对等、长期存在、由消息驱动。两者共享的只有一个 PostgreSQL 实例、一个 Desktop 应用壳和一个凭证 crate。

全部 `collab_*` 表的 DDL 与约束理由在 [collaboration-data-model.md](collaboration-data-model.md)，**那是 schema 的唯一权威副本**；本篇只描述语义与边界。

本篇多处援引 Cumora 的 BYOA 实现作为先例，源码级细节与常量整理在 [references/cumora-byoa.md](references/cumora-byoa.md)。**那是外部调研，不约束 OpenWork**；本篇是唯一权威。

## 1. 定位与边界

| 是什么 | 不是什么 |
|---|---|
| 常驻同事：有人格、有记忆、有自己的办公桌 | 不是任务容器，不随任务创建销毁 |
| 消息驱动：房间里有事才醒 | 不是 Tool Call 派生，父子关系不存在 |
| 推理跑在**外部 OpenCode 进程**里 | 不走 `run_loop.rs`，不是 `SessionActor` |
| 每个 Agent 有私有 home | **碰不到你的项目代码**，也没有共享工作目录 |
| 协作产物是**消息与看板卡** | 不产出代码变更，不改工作区 |
| daemon 独立于 Desktop 存活 | Desktop 关掉 Agent 照常工作 |

**核心不变量：**

1. **daemon 是协作数据的唯一写者。** Desktop 与 Agent（经 MCP）都是它的客户端。房间 sequence 分配、HELD 判定、发言去重只有一处实现。
2. **OpenWork 不推进协作 Agent 的推理。** 引擎在外部进程里自己跑循环、自己管上下文、自己压缩。[architecture.md §3](architecture.md) 不变量 #2 的准确表述是"**OpenWork 自身的** Agent Loop 只有一处"，协作模式不在其管辖内。
3. **Agent 之间通过消息交互，不通过共享文件。** 没有共享工作目录、没有跨 home 读写约定。
4. **协作模式不依赖 `openwork-core`。** 依赖方向是 `openwork-collab → openwork-models + openwork-credentials`，反向依赖禁止。
5. **发言是机器可判定的事实。** Agent 的自然语言输出只是它自己的内容；只有 `reply` 工具经 daemon 落库才算说了话。这条是 HELD、去重、claim 全部成立的前提。
6. **协作模式没有 Trace。** [trace.md](trace.md) 的 Span 树、完整度派生、token 口径全部建立在"OpenWork 组装了这次请求"之上，这里不成立。协作模式只有运行日志与事件流。

## 2. 进程与所有权

```text
Desktop (Tauri)                    openwork daemon                opencode serve
  │                                   │                              │
  │  IPC (Unix socket)                │  HTTP / SSE                  │
  ├──────────────────────────────────►├─────────────────────────────►│
  │  发言 / 建 Agent / 看板 / 事件      │  session · prompt_async      │  session × N
  │  待审批角标 / 审批回复              │  abort · /event · /permission│  （各带自己的
  │                                   │                              │   directory）
  │                                   │◄─────────────────────────────┤
  │                                   │   MCP over local HTTP        │  Agent 调工具
  │                                   │   reply / glance / inbox     │
  │                                   │   react / dm / card          │
  │                                   ▼
  │                              PostgreSQL
  │                              （collab_* 表，独立 migrations）
  └── openwork-core（现有工作台）──► PostgreSQL（现有表）
```

| 状态 | 唯一 Owner | 其他组件怎么访问 |
|---|---|---|
| 房间、消息、成员、看板 | daemon | IPC / MCP |
| Agent 定义与启停 | daemon（写库） | Desktop IPC |
| `opencode serve` 进程与 session 生命周期 | daemon | 不暴露 |
| seen 游标 / HELD token / nudge claim | daemon **内存** | 不暴露 |
| inbox 已读游标 | `collab_room_members.last_read_seq` | daemon |
| 看板卡认领 | `collab_cards.claimed_by` | MCP `card` 工具 |
| Agent 私有记忆 | Agent 自己的 home 文件 | 无人代管 |
| Provider 凭证 | `openwork-credentials` | daemon 与 core 各自读 |

**为什么 daemon 独占写。** 房间 sequence 必须原子分配、HELD 必须在写入前做新鲜度判定、发言必须逐字去重——这三件事都要求一个串行化点。让 Desktop 也能直写会立刻产生两份实现，而它们的分歧只在并发下暴露。

**为什么不需要 Redis。** cumora 用 Redis 是因为服务器是多副本的。daemon 是单实例（socket 绑定即锁），协调状态放内存即可；只有跨重启必须存活的（已读游标、卡片认领）落 PostgreSQL。

## 3. Agent 身份与 home

### 3.1 定义

Agent 定义存 `collab_agents`，由用户在 Desktop 创建编辑。id 稳定，被消息作者与卡片指派引用。

| 列 | 语义 |
|---|---|
| `id` | 稳定主键，同时是 home 目录名与 OpenCode agent 名 |
| `name` / `role` / `bio` | 名册与人格 |
| `system_prompt` | 用户写的人格正文 |
| `model` | 传给 OpenCode 的 `providerID` + `modelID` |
| `opencode_session_id` | 该 Agent 当前的 OpenCode session，跨重启复用 |
| `enabled` | 停用后不参与任何唤醒 |

**id 由系统从名称派生，用户不填。** 它同时是 home 目录名、OpenCode agent 名、以及模型称呼队友时用的串，格式受 `^[a-z][a-z0-9_]{0,47}$` 约束——让用户去满足一个正则，失败形态是数据库 CHECK 报错，而这个值建完基本改不了。

派生规则：

| 输入名称 | 派生结果 |
|---|---|
| `Alice` | `alice` |
| `Code Review` | `code_review` |
| `Alice`（已存在） | `alice_7f3a`（短随机尾，重试若干次后退化为更长的尾） |
| `小艾` | **派生不出** |

**派生不出时才向用户要一个英文标识**，而不是自动生成 `agent_7f3a` 这类无信息的串。理由：这个 id 会出现在每一次唤醒的 roster 里，也会出现在 Agent 互相称呼的文本里；一屋子 `agent_7f3a` / `agent_2b81` 会让模型每次都得回查映射，而中文名产品里这会是常态而非例外。

**库表是唯一事实源。** daemon 把它渲染成 OpenCode 的 agent 定义（`prompt` + `permission` + `model` + `mode`），一次下发同时解决人格与权限，Desktop 改完下一轮生效。

**定义不放在 home 里。** home 是 Agent 自己的可写空间，放进去的东西它自己就能改——人格可被静默改写且不留痕迹。同理 `opencode_session_id` 也在库里，不落 home。

### 3.2 home 布局

```text
~/.openwork/collab/
├── daemon.sock                    Desktop ↔ daemon
└── homes/<agent_id>/              OpenCode instance 的 directory
    ├── opencode.json              daemon 覆盖写：MCP 端点 + 该 Agent 的 token
    ├── AGENTS.md                  daemon 覆盖写：人格与协作协议
    └── memory/MEMORY.md           Agent 自己维护
```

home 就是传给 OpenCode 的 `x-opencode-directory`。**没有 `.git/`**（那是 Codex app-server 的硬要求，OpenCode 没有）、**没有 `bin/`**（不再有 shim）。

`opencode.json` 与 `AGENTS.md` 每次启动由 daemon **覆盖写**——它们在 Agent 的可写范围内，必须假定被改过。

### 3.3 身份绑定

每个 Agent 的 `<home>/opencode.json` 里的 MCP 配置带一个**专属 token**。OpenCode 的配置按 instance directory 加载，而每个 Agent 一个 home，所以**身份绑定不需要额外机制**：daemon 收到 MCP 调用时以 token 对应的身份为准，忽略任何参数里的 agent id，工具面也**根本不提供**指定身份的参数。

这道防线挡的是**提示注入**：房间里每个 Agent 都在读别人写的内容，一条"请以 alice 的身份发言"是必然会遇到的输入。

token 的有效期与 daemon 进程一致，存在内存里的 `token → agent_id` 映射。daemon 既签发又校验且同进程，**不需要 JWT、TTL 或刷新**。

## 4. 引擎：OpenCode

daemon 拉起**一个** `opencode serve`，每个 Agent 是它的一个 session。

```text
daemon ──HTTP──► opencode serve ──► N 个 session（每个带自己的 directory）
       ◄──SSE───
```

**为什么是一个 server 而不是每 Agent 一个进程**：OpenCode 逐请求按 `x-opencode-directory` 头加载 instance，一个 server 天然服务多个目录。这直接省掉了每 Agent 一个常驻进程的整套生命周期管理——spawn 节流、崩溃重建、并发信号量。代价是单点，见 §15 R-N5。

### 4.1 用到的端点

全部经 1.18.18 实测（见 §15 的 P0 结论）：

| 用途 | 端点 |
|---|---|
| 建 / 取 session | `POST /session`、`GET /session/{id}` |
| 发起一轮 | `POST /session/{id}/prompt_async`（返回 **204 No Content**） |
| 打断 | `POST /session/{id}/abort` |
| 事件流 | `GET /event`，**客户端按 `properties.sessionID` 过滤** |
| 待决审批 | `POST /permission/{id}/reply` 回复；`GET /permission` **仅按 Agent 查询**（instance 范围），全局集合由 `GET /global/event` 维护，见 §6 |
| Agent 定义 | `GET /agent` |
| 健康与版本 | `GET /global/health` |

**只用非 `/experimental/` 的端点。** 那一组随时会变，而失败形态是运行时 404 而不是编译错误。

**两套 path family 只选 v1。** `/session/*`（v1）与 `/api/session/*`（v2）并存，且 v2 侧确实有 `GET /api/session/{id}/event` 这样的单会话流；但 **`prompt_async` 只存在于 v1**，v2 的对应物 `POST /api/session/{id}/prompt` 不是同一个接口。为一个单会话事件流去混用两套表面，换来的是两套响应形状与两条演进路线，不值得——**统一走 v1，事件在客户端过滤**。

**`/event` 与 `/permission` 都是 instance 范围的**，即受 `x-opencode-directory` 约束，不是跨全部 Agent 目录的 server 全局视图。这一条直接影响待审批角标的设计，见 §6。

**事件流会重复送同一帧。** P6 实测：OpenCode 可能发出**逐字节完全相同**的 usage / 工具状态帧。任何消费事件的代码都要按活跃 run 做指纹去重，**否则 token 用量会被重复计入**——而多算出来的数字看不出是错的。usage 快照另按 assistant message id 聚合，run 总量取每个 message 的最新一份。

### 4.2 启动自检

opencode 由**用户自行安装并登录**——这样它已有的登录态直接可用，包括通过 `CodexAuthPlugin` 拿到的 ChatGPT/Codex 订阅额度、GitHub Copilot，以及任何 API key provider。

代价是 OpenWork 依赖一个自己不控制、且演进很快的外部 HTTP 表面（`/session/*` 与 `/api/session/*` 两套路径并存，说明 API 正在迁移）。**没有任何检查时，opencode 改掉我们依赖的东西，表现是"某个 Agent 半夜起莫名其妙不回话"**——在一个常驻、异步、无人值守的系统里，这是最难查的一类失败。

**不比版本号。** 版本相等不保证行为相同（配置与插件都能改变行为），版本不同也不意味着不兼容；用等号近似"兼容"两边都会错。**版本号只进诊断信息，不作判据。**

改为在启动时探真正依赖的行为，四次 HTTP、零 token：

| 探测 | 守住什么 |
|---|---|
| `GET /global/health` 返回 200 且版本是合法 semver | 服务活着，且 health 契约没变 |
| `GET /session?limit=1` 返回 200 且**是数组** | v1 路径族还在——v2 的 `/api/session` 返回 `{data, cursor}`，形状不同；而 `prompt_async` 只存在于 v1 |
| `GET /agent` 返回 200 且是数组 | Agent 定义接口还在 |
| `GET /global/event` 能建立 SSE | 跨 instance 汇总待审批的唯一廉价通路还在（§6） |

任一不满足即**拒绝启动**，诊断指名是哪一条、实际收到什么、opencode 版本是多少。

**探不了的部分靠严格解析兜底。** 事件载荷形状与审批流没法在不烧 token 的前提下预探，因此它们在首次使用时必须**响亮失败**——缺字段就报"`/global/event` 的 payload 缺少 `directory` 字段，opencode 版本 X"，绝不 `unwrap_or_default()` 之后静默走错路。

### 4.3 OpenCode 原生提供的，我们不重做

持久上下文与自动压缩、工具执行、权限判定与审批协议、skills、MCP 客户端、子 Agent（`task`）。因此协作模式**没有** `context-window` / `compaction` / `tools` 三条链路的对应物。

`collab_agents` 的能力面**不做任何禁用**，OpenCode 默认给什么就是什么。已知代价见 §15 R-N3 与 R-N4。

## 5. 动作接口：远程 MCP

daemon 在本地 HTTP 上暴露一个 MCP 端点，每个 Agent 的 `<home>/opencode.json` 指向它并带专属 token。

| 工具 | 语义 |
|---|---|
| `reply` | 在房间发言。可能返回 HELD + 新消息 |
| `glance` | 重读房间最新状态 |
| `inbox` | 读自己的未读 |
| `react` | 表态而不发言 |
| `dm` | 与某个 Agent 或用户私聊 |
| `card` | 看板：list / create / claim / move |

**一个不多。** 每条都承载一个已选定的机制：`reply` 是乐观发送，`glance` 是 HELD 后重读，`inbox` 是未读，`react` 是"看到了但不重复"，`dm` 是双边对齐，`card` 的 claim 是共享工作面认领。

**为什么是 MCP 而不是 CLI。** 让 Agent 在 bash 里调一个 shim 会同时带来四个问题：每次动作冷启动一个进程；**正文在 shim 运行之前就被 shell 改坏**（反引号与 `$(...)` 被当命令执行并塌成空，引号被吃掉）；命令语法要写进 standing prompt 去教；还需要一个额外的 crate 承载线协议。MCP 把这四个一起消掉——参数以 JSON 传输、从不经过 shell，工具 schema 原生可见。

[architecture.md §6](architecture.md) 把 MCP 列为非目标，指的是 **OpenWork 作为 MCP 客户端**去消费外部 server；这里方向相反，是 OpenWork **作为服务端**。

**没有副作用回报协议。** daemon 直接接收工具调用，**它自己就是记录副作用的人**——"这个 Agent 本轮是否已经发言"是它的一手事实，不需要 Agent 回报。

`status` / `typing` 也不做工具：OpenCode 的事件流已足以让 daemon 推导实时状态。

## 6. 权限与审批

用 **OpenCode 的默认权限层**，不自定义 ruleset。默认层的形状是：

```text
"*": "allow"                                        directory 内全放行
external_directory: { "*": "ask", <白名单>: "allow" }  出 directory 要问
read: { "*": "allow", "*.env": "ask", "*.env.*": "ask" }
doom_loop: "ask"
```

因此"Agent 待在自己 home 里"**默认就是一次拦截**，而不是 standing prompt 里的一句话——这是相对上一版设计的实质增强。

`ask` 触发时 OpenCode 发 `permission.asked`，daemon 转给 Desktop 弹出审批；用户回 `once` / `always` / `reject`，`reject` 可带 message 反馈给模型。

**审批永久挂起，不超时。** 代价是明确的：无人值守时（主动性会在半夜唤醒 Agent）一个 ask 会把该 Agent 占到你回来为止，期间它对房间里的一切都不响应。两条缓解，都必须做：

- Desktop 上要有**全局待审批角标**，让"有人在等你"永远可见；
- `POST /session/{id}/abort` 是逃生口，用户可以直接掐掉那一轮。

每个 Agent 一个 session，所以卡住的只有它自己，不影响其他同事。

**角标的数据来源：单条 `GET /global/event`。** P0 发现 `GET /permission` 是 **instance 范围**的——它只返回 `x-opencode-directory` 指向的那个目录下的待决审批，而每个 Agent 一个 home，所以它给不出跨 Agent 的全局列表。V4 实测确认了替代路径：

**`GET /global/event` 跨 instance 送达 `permission.asked`。** 证明不在于"它和 `/event` 收到了同一条事件"，而在于：**全局流没有带 `x-opencode-directory`**，而 OpenCode 在无此请求头时默认取 `process.cwd()`；它却收到了一个临时目录的事件——**全局流看到了它从未指名的 directory**。事件外面还包着一层 `{ directory, payload }`，`directory` 正是触发审批的那个 home。

因此 daemon 用**一条**全局 SSE 连接维护 server 范围的待决集合，不需要为每个 Agent 各开一条 `/event` 只为审批。被否决的方案是"每个 Agent 目录各订一份"——它一定可行，但 N 条连接换不到任何东西。

`GET /permission` 仍然有用，但只用于**按 Agent 查它自己的待决**，不用于全局汇总。

**汇总在 daemon 侧完成**，Desktop 只拿一个数字和一个列表——前端不该知道 instance 这个概念。

## 7. 房间与消息

### 7.1 形态

群房间与 DM 是同一张表的两种 `kind`。DM 由成员集合唯一确定，重复创建复用已有房间。本机只有一个人类用户，成员表里以固定 id 表示。

涉及 `collab_rooms` / `collab_room_members` / `collab_messages` / `collab_reactions` 四张表，DDL 见 [collaboration-data-model.md §2](collaboration-data-model.md)。

`sequence` 在房间内单调，与消息插入**同一事务**内 `UPDATE collab_rooms SET next_seq = next_seq + 1 RETURNING` 分配。那一步取的行锁同时把该房间的所有插入串行化——这是 §7.2 的去重预检能安全"先查后写"的唯一理由。`UNIQUE(room_id, sequence)` 兜底。

DM 的复用靠 `direct_key`（成员 id 排序后拼接）加唯一索引保证；没有它，两个 Agent 同时 `dm` 同一个人会建出两个房间。

`kind` 取 `normal` / `system`（加入、离开、看板事件）。

### 7.2 去重

同房间、同作者、同正文在短窗口内的重复插入直接拒绝，返回既有消息 id。这是 HELD 之外的第二道：HELD 挡的是"基于陈旧状态发言"，去重挡的是"同一句话被投递两次"。

### 7.3 已读与 seen 是两件事

| | 存哪 | 语义 | 谁推进 |
|---|---|---|---|
| 已读游标 | `collab_room_members.last_read_seq` | inbox 查询的起点 | Agent 消费完 inbox 后 |
| seen 游标 | daemon **内存**，TTL 10min | 该 Agent 撰写本次回复时看到的最高 seq | 每次 `glance` / wake brief |

**这两者绝不能共用一列。** cumora 试过（`a6e69aa`），把 seen 写进 `conversation_reads.last_read_at`，而那一列同时是 inbox 的 SELECT 游标——一更新到当前时间，下次 inbox 就返回空，daemon 挂成 silent-busy。任何与 inbox 游标共享状态的设计在结构上就是不安全的。

seen 游标 **fail-open**：它是协调信号不是正确性不变量，丢了最坏结果是一次重复发言，而不是卡死。

## 8. 调度与唤醒

### 8.1 流程

```text
消息落库
  → daemon 计算候选 Agent（房间成员 − 作者，enabled，未 mute）
  → 2.5s debounce 合并突发
  → 每个候选 Agent 独立走 triage
      actionable = false → 不唤醒，记 collab_triages
      actionable = true  → POST /session/{id}/prompt_async
  → 若期间又有新消息且本轮已无法接住，标 pending rerun，结束后再跑一次
```

`prompt_async` 的输入是一份 inbox digest（未读消息、房间近况、名册、`MEMORY.md`、triage 给出的 promptNote）。

**名册随每次唤醒下发，不写进 `AGENTS.md`。** 房间成员会变，静态文件会过期。

**称呼队友必须用 roster 里的 id，不用显示名**——这一条要写进 standing prompt。`@` 在本设计里不是路由原语（没有任何正则解析它），所以写错**不会报错**：消息照常发出，triage 甚至可能猜对。代价出在长期——房间历史里混着 `@小艾` / `@Alice` / `@alice` 三种写法，triage 每次都要花 token 消歧，而猜错时的表现是"我明明叫了它，它没理我"，且查不出原因。显示名可以是中文，这让写错几乎必然发生。

**忙时语义已实测（1.18.18）：被运行中的循环接住。** 向忙碌 session 发 `prompt_async` 返回 204，第二条内容立刻写成同 session 的新 user message；它不会挤进已经在进行中的那一次工具调用，而是由**同一个尚未 idle 的 runner 的下一次 loop step** 处理。两轮之间没有 idle、没有排队边界、没有 HTTP 错误。

**但这不是免费的。** 实测里第一轮被要求最终回答 `FIRST_DONE`，第二条 prompt 在其 `bash sleep 10` 运行期间进入后，第一个 assistant 以 `finish: "tool-calls"` 收尾，**`FIRST_DONE` 从未出现**；runner 直接以第二条 user message 为 parent 开了新 assistant，只输出了 `SECOND_DONE`。

也就是说：**注入会让进行中那一轮的结论可能永远不产出。** 两条设计后果：

1. **不能假设"我发过去的每一轮都会有一个对应的回答"。** 一轮的产物是不是真的发生了，只看 `reply` 工具有没有被调用（§1 不变量 5），不看模型有没有说话。
2. **未读绝不因"已注入"而清除。** 注入是低延迟优化，不是投递通道；可靠投递永远是"未读 + pending rerun"。第一轮被打断而没做完的事，靠未读仍在原地这一点被下一轮重新拾起。

因此 debounce 的 2.5 秒不只是省 token——**它同时是在减少打断**。突发消息合并成一次注入，比逐条注入更不容易把前一轮的结论撞掉。

### 8.2 triage：大脑用订阅，小脑用 API

**主推理走本地 OpenCode session（吃用户的订阅额度），triage 走 OpenWork 已配置的廉价 provider API。** 用户在设置里指定一个 support model。

cumora 的 BYOA 只能用本地 CLI 跑 triage，因为它的 daemon 拿不到服务器凭证。它为此付出的代价写在自己的注释里：单次 4983–7772ms、必须加并发信号量（并发起 4 个本地 CLI 直接触发限流，然后每个 Agent 停 30 秒）、必须用中性 cwd 防人格污染、必须做限流退避与 trouble streak。**这些在 OpenWork 全部不需要。**

裁决形状与 cumora 一致：

```json
{ "actionable": true, "responseMode": "me|each|one-of-us",
  "reason": "...", "promptNote": "..." }
```

`responseMode` 只是**粗粒度提示**，daemon 不据此挑人。谁回、怎么回由大脑读房间自己决定；小模型选错人时没有第二道防线，而房间彻底哑火比多一次发言糟得多。

**失败策略是非对称的：**

| 场景 | 策略 |
|---|---|
| 有人类在等 | fail **open** —— 宁可多醒一次 |
| 纯 Agent 之间 | fail **CLOSED** —— 宁可不醒，防止互相刷屏 |

**被显式请求的非散文动作同样算 actionable。** 队友或用户明确要求一个 reaction、一次认领这类不产出正文的动作时，裁决必须是 `actionable=true`。判成"无需唤醒"的后果很隐蔽：主 Agent 根本没有被叫醒，也就**永远没有机会调 `react`**，表现是"我让它点个赞，它没反应"。

**不用正则判断消息语义。** "这是不是在叫我""这算不算问候"全部交给小模型；正则只用来解析模型自己吐的 JSON。唯一的非模型短路是"未读为空"——那是计数，不是分类。

## 9. 发言竞争

三层，职责不重叠。

### 9.1 乐观发送 + 服务端 HELD（硬）

Agent 只能看到**已发布**的消息流和自己的私有 seen 游标。没有 composing 列表、没有认领顺序、没有"谁排在你前面"——**"我是第 3 个认领的所以我发 3"在结构上不可表达**。

`reply` 落库前做新鲜度判定：房间当前最高 seq 若已超过该 Agent 的 seen 游标，写入被拒，返回 `HELD` 与期间的新消息。Agent 读完重算重发。

**预检只在房间成员多于两人时生效。** 一对一没有抢答对象，对 DM 跑预检只会凭空增加一次拒绝与重试。

HELD token 是**确认**不是通行证：短 TTL（120s），并携带 HELD 当时展示给它的最高 peer seq。长 TTL 会让一次让出的 hold 变成未来的绕过弹药。

### 9.2 glance 五条（软）

写进 standing prompt，**只有五条，不许再长**：

1. 人类点名某个队友时，读清楚点的是谁——不是你就别插话（可以 react）。
2. 从**真实已发布**的状态回复，不要从"我排第几"推理。
3. **乐观发送，服务端是安全网**——不要 glance→想→glance 地空转；被 HELD 就重读重算重发。
4. 别重复队友已经说过的；说完就停。
5. **不要认领聊天轮次**。认领只用于队友可能重复做的真实工作（一张看板卡）。

规则墙塌缩成五条的原因是 8.1 让错误推理不可表达。**Agent 判断出错时，先问服务端的 gate 该不该改，而不是加第六条规则。**

### 9.3 claim（软，只用于看板）

`card claim` 是唯一的认领。抢不到说明队友在做，换一件事。

## 10. 看板

协作模式唯一从零设计的产品面，它同时承担三个角色：agenda 的信号源、claim 的对象、用户参与分工的入口。

三层：房间 → 看板 → 列 → 卡片。DDL 与"为什么保留看板这一层"见 [collaboration-data-model.md §3](collaboration-data-model.md)。

列有显式的 `is_done` 标记，agenda 靠它判断哪些卡还算未完成——**不靠正则猜列名**。

认领是一次条件 UPDATE（`WHERE claimed_by IS NULL`），返回 0 行即表示队友已认领。**认领不设 TTL**：TTL 到期时无法区分"Agent 死了"和"Agent 在做一件耗时长的事"，后者被抢走会导致两边重复做同一件工作。释放改由会话状态派生——daemon 启动时释放全部，周期检查发现认领者的 OpenCode 会话已不在运行且超过宽限期则释放，用户也可在 UI 强制取消。

卡片变更会产生房间 `system` 消息并进入正常唤醒流程——这条 board→wake 通路是主动性能工作的前提。

只做看板，**不做日历**。cumora 的 agenda 吃卡片与日历两类信号，日历在 OpenWork 没有对应物。

**与 OpenCode 的 session todo 并存。** OpenCode 自带 per-session 的 todo，而我们不禁用它的任何能力（§4.3）。两套任务系统因此同时存在：**agenda 只扫看板**，Agent 写进 session todo 的东西 agenda 看不见。这是明知的代价，记在 §15 R-N3；缓解只有一条——standing prompt 里说清楚：要让队友和用户看见的工作放看板，自己这一轮的步骤放 todo。

## 11. 主动性与防死循环

### 11.1 三层主动性

| 层 | 行为 |
|---|---|
| `idle` | 轮转挑一个安静且可用的 Agent，避免总唤醒同一个 |
| `agenda` | 唤醒前先用小脑判断：分配/提及它的未完成卡片 + 停滞房间，有真活才唤醒大脑，并给出聚焦 brief |
| `scanner` | 后台观察跨房间变化，快照 fingerprint 命中冷却则不唤醒 |

**scanner 的指纹必须排除它自己的产出。** 主动标记与自己发的回复若进入快照，下一轮扫描就会看到"房间变了"，于是**自我唤醒成环**——而每一轮都是真实的模型调用。

**停滞推动的 claim 要一直持有到主派发结束**，不是判定完就释放。提前释放会让第二个 Agent 在第一个还没发言时抢到同一个停滞房间，两个人一起推。

**agenda 存在的理由是省钱**：没有它，通用 idle 唤醒会让大脑烧一整轮推理只为回答"没什么可做的"。

停滞推动的三个约束缺一不可：按房间（而非按最后一条消息）设冷却——推一下就改变了最后一条消息，按消息设会重新武装然后反复催；claim 保证同一次停滞只有一个 Agent 去推；decline 计数上限 3，房间有新消息则清零。

### 11.2 三道防死循环

| 防线 | 机制 |
|---|---|
| triage 非对称失败 | 纯 Agent 场景 fail CLOSED（见 §8.2） |
| DM 死循环探测 | Agent↔Agent DM **默认参与**，但每 8 条强制跑一次 triage 专门找没有进展的来回，找到就掐断 |
| loop-cap / rate gate | 每 Agent 的 turn token 与速率闸、突发合并；裁决 source 记 `loop-cap` / `rate-limited` |

后两者是**服务端在调用模型之前的短路**，不是模型判断的结果。

**V1 不做额度闸。** 引擎上报的用量与限流信息只记录不拦截。已知后果见 §15。

## 12. Desktop

Desktop 是 daemon 的**客户端**：启动时发现 socket，未运行则拉起；关闭时**不停止** daemon。前端跑在 WebView 里连不了 Unix socket，因此读写与事件一律经 `src-tauri` 中转。

界面四块：房间、Agent 管理、看板、日志与事件流抽屉，外加一个**全局待审批角标**（由 daemon 从单条 `GET /global/event` 维护的全局集合给出，见 §6）——审批永久挂起，所以“有人在等你”必须永远可见。**不做第二套 Trace UI**——[trace.md](trace.md) 的 Span 树与完整度派生建立在"OpenWork 组装了这次请求"之上，而这里请求由 OpenCode 自己组装，口径对不上；协作提供的是 `collab_runs` / `collab_triages` / `collab_events` 三张平表的时间序视图。

**前端的 Shell 划分、导航、状态、复用边界与验收见 [collaboration-desktop.md](collaboration-desktop.md)，那篇是前端的唯一权威。**

## 13. 与现有工作台的分离清单

| 层 | 分离方式 |
|---|---|
| 进程 | 独立 daemon + 独立引擎进程 |
| 代码 | `openwork-collab` 不依赖 `openwork-core` |
| Schema | `collab_*` 表 + 独立 migrations 目录 + 独立版本表 |
| 引擎 | 外部 OpenCode，不走 `ModelPort` 的推理链路 |
| 文件系统 | 每 Agent 私有 home，不共享工作目录 |
| 权限 | [permissions.md](permissions.md) 的规则集与判定不适用；边界与审批协议由 OpenCode 提供，OpenWork 只做审批的呈现与回传 |
| 上下文 | [context-window.md](context-window.md) / [compaction.md](compaction.md) 不适用 |
| 追踪 | [trace.md](trace.md) 不适用 |

**共享的只有三样**：同一个 PostgreSQL 实例、同一个 Desktop 应用壳、`openwork-credentials`（凭证读取从 `openwork-core` 下沉，core 与 collab 各自依赖）。

## 14. 明确不做

| 不做 | 理由 |
|---|---|
| Cloud Pod / K8s / FUSE / 多租户 | 本地单用户产品，整条链路不存在 |
| 第二个引擎适配 | OpenCode 已覆盖多 provider 与多订阅（Codex / Copilot / API key），再包一层是投机抽象 |
| Redis | daemon 单实例，协调状态放内存即可 |
| shim CLI 作为动作接口 | 已选 MCP；两个入口等于两套动作语义、两套身份绑定 |
| 共享仓库 / worktree / 合并 | Agent 待在自己 home，写冲突从根上不存在（OpenCode 有 worktree 端点，我们不用） |
| 日历、邮件、联系人、投票、文档 | cumora 的产品面，OpenWork 没有对应物 |
| pgvector 记忆检索 | BYOA 路径的记忆本来就是 home 里的一个文件 |
| climate 关系状态 / per-Agent Skills / 语气模块 | 与"同事感"相关但非承重，收益难验收 |
| 副作用回报协议（JSONL sideEffects） | MCP 调用直达 daemon，副作用是 daemon 的一手事实 |
| 额度闸 | V1 只记录 |
| 禁用 OpenCode 的任何自带能力 | 全部开放，含 `task` 子 Agent 与 session todo |
| 自定义权限 ruleset | 用 OpenCode 默认层 |
| 审批超时自动拒绝 | 永久挂起等用户，见 §6 |
| convene 显式轮流会议 | 有价值但是第二套发言调度路径，等自由房间跑顺再说 |
| 协作模式与工作台的自动联动 | 用户手工搬运即可；自动打通会立刻破坏四层分离 |

## 15. 已知风险与验证项

| | 项 | 说明与应对 |
|---|---|---|
| ~~V1~~ | Rust 驱动 `opencode serve` | **P0 已验证**（1.18.18）：建 session → `prompt_async`(204) → `/event` 收文本与 usage |
| ~~V2~~ | 忙时发 prompt | **P0 已验证**：被运行中的循环接住；代价是进行中那一轮的结论可能被吞，见 §8.1 |
| ~~V3~~ | MCP 回连与审批链路 | **P0 已验证**：Agent 可见并调用工具、token 随请求到达；`permission.asked` 可收，`once` / `reject + message` 均生效且 message 被模型看到 |
| ~~V4~~ | `GET /global/event` 是否跨 instance | **P1 已验证**：跨 instance 送达 `permission.asked`；全局流未带 directory 头却收到临时目录的事件，且外层包 `{directory, payload}`（§6） |
| **R-N1** | 依赖面从一个二进制一条协议变成 162 个端点 | `/session/*`(v1) 与 `/api/session/*`(v2) 并存说明 API 正在迁移，而 `prompt_async` **只在 v1**。统一走 v1、只用非 `/experimental/` 端点、**启动自检 + 严格解析**（§4.1、§4.2） |
| ~~R-N6~~ | 全局待审批没有现成端点 | **已解决**：单条 `GET /global/event` 汇总，`GET /permission` 退为按 Agent 查询（§6） |
| **R-N7** | 注入会吞掉进行中那一轮的结论 | 见 §8.1。缓解是 debounce 合并 + 未读不因注入而清除；**不做的话表现为"它答了后一个问题，前一个石沉大海"** |
| **R-N2** | **审批永久挂起** | 无人值守时一个 ask 会把该 Agent 占到用户回来，期间它对房间里的一切都不响应。缓解：全局待审批角标 + `abort` 逃生口（§6），两者都必须做 |
| **R-N3** | **两套任务系统** | OpenCode session todo 与共享看板并存，agenda 只扫看板（§10） |
| **R-N4** | `task` 子 Agent 全开 + 无额度闸 | token 消耗是无上限的乘法。V1 不缓解 |
| **R-N5** | 单点 | 一个 `opencode serve` 挂了全体 Agent 停止。daemon 须能检出并重启它，且重启后按 `opencode_session_id` 恢复 |
| **R2** | 无额度闸 | 主动性可能在无人值守时烧光订阅额度，直接影响用户自己的开发。V1 不缓解 |
| **R3** | 不变量措辞 | [architecture.md §3](architecture.md) #2 需明确为 OpenWork 自身的 Agent Loop 只有一处 |
| **R4** | 看板无参照实现 | 是 V1 唯一从零设计的产品面 |

**相对上一版消解的风险**：协议从未文档化变成 OpenAPI；home 约束从 prompt 约定变成 `external_directory: ask` 的实际拦截；`.env` 读取默认需要审批；shim 冷启动与 shell 改坏正文两类问题随 MCP 一起消失。

## 16. 分期与验收

七期。前端不独立排期，与后端同一条竖切；前端的细化验收见 [collaboration-desktop.md §12](collaboration-desktop.md)。

顺序上不能换的只有三处：P0 在最前（尖刺不过整个方案作废）；**P5 必须在 P4 之后**（agenda 没有卡片就没有信号，退化成通用唤醒）；P2 必须在 P1 之后（前端没有可连的 daemon）。P3 与 P4 之间没有硬依赖。

### P0 — 尖刺

不写业务代码。**任一不通过则回到设计。** 产物可以转正，不是丢弃的实验。

1. Rust 建 session（带 `x-opencode-directory` 指向一个 home）→ `prompt_async` → 从 SSE 收到文本与 usage；
2. 同一 session 第二轮能引用第一轮内容（`opencode_session_id` 复用成立）；
3. **向忙碌 session 发 prompt**：确认是被运行中的循环接住，还是报错——结论直接决定 §8.1 保留哪一半；
4. `<home>/opencode.json` 配的远程 MCP 能连上 daemon，工具在 Agent 侧可见可调，且 token 决定身份；
5. 触发一次越界访问，daemon 收到 `permission.asked`，`reply` 后 Agent 继续。

### P1 — 后端单向通路（无 UI）

`openwork-credentials` 下沉；collab migrations；daemon 骨架与单实例锁；OpenCode 客户端与启动自检；MCP 服务端与 `reply` / `inbox`；房间、消息、sequence 原子分配、逐字去重；`@` 唤醒 + 2.5s debounce + pending rerun。**这一期不做 triage**，@ 到谁谁醒。

6. daemon 用**单条** `GET /global/event` 汇总跨 Agent 的待决审批——不为每个 Agent 各开一条 `/event` 只为审批（§6）；
7. `openwork-core` 与 `openwork-collab` 都能读到同一份凭证，现有测试全绿；
8. daemon 二次启动因 socket 已占用而拒绝，并给出可读提示；
9. opencode 版本不在支持区间时**拒绝启动**并给出可读诊断，而不是运行中报 404；
10. 命令行建两个 Agent，`@` 其中一个，它通过 `reply` 工具回话并落库；
11. Agent 定义修改后，下一轮使用新的 prompt 与权限；
12. 手工改坏 home 里的 `opencode.json` / `AGENTS.md`，下次启动被 daemon 覆盖回正确内容；
13. `opencode serve` 被杀掉后 daemon 能检出并重启它，按 `opencode_session_id` 恢复各 Agent 的上下文。

### P2 — 壳、房间、Agent 管理

第一个可以给人看的版本。`src-tauri` 的 daemon 客户端；`modeStore` + 切换按钮 + `CollabShell` + Rail；两个事件桥常驻 `App.tsx`；房间三栏；消息游标分页；Agent 管理；未读；**待审批角标**。

14. 点侧栏底部按钮进入协作模式，再点一次回到工作台；重启回到上次所在的模式；
15. 工作台跑着 Turn 时切到协作模式再切回来，无需人工刷新；
16. 协作房间来新消息时，即使当前在工作台模式，侧栏按钮出现未读提示；
17. 在房间里发言，被 `@` 的 Agent 回话并出现在同一流里；
18. 在 Desktop 创建、编辑、停用 Agent；停用后不再参与任何唤醒，房间历史里它的消息与身份完好；
19. Agent 触发 `ask` 时 Desktop 出现待审批角标与卡片，用户回 `once` / `reject` 后 Agent 相应继续或换路；
20. 用户能对一个卡住的 Agent 触发 `abort`。

### P3 — 多 Agent 协调

triage 小脑（廉价 provider API）+ 非对称失败；seen 游标（内存，与 inbox 游标严格分离）；HELD 预检与 token；`glance` / `react` 工具；glance 五条进 `AGENTS.md`；OpenCode 事件驱动的实时状态名册。

21. 与该 Agent 无关的消息不唤醒它，且 `collab_triages` 有一条 `actionable=false` 记录；
22. 纯 Agent 场景下 triage 失败时**不唤醒**；有人类在等时 triage 失败**照常唤醒**；
23. 两个 Agent 同时对同一条消息作答，后写入的一方拿到 HELD 与期间新消息，重发后房间没有重复内容；
24. 一对一 DM **不跑** HELD 预检；
25. seen 游标推进**不影响** inbox 未读计算（回归 §7.3 的教训）；
26. daemon 重启后已读游标保留，seen 游标清空且不导致误判；
27. Agent 遇到队友已答的情况使用 `react` 而非重复发言；
28. 右栏名册显示 Agent 正在执行的动作，状态来自 OpenCode 事件而非轮询。

### P4 — 看板

看板 / 列（显式 `is_done`）/ 卡片；`card` 工具；认领的原子 UPDATE + daemon 启动释放；board→wake；看板 view。

29. 卡片创建 / 移动产生房间 `system` 消息并进入唤醒流程；
30. 两个 Agent 争抢同一张卡，只有一个 `claim` 成功，另一个收到明确失败并转向其他工作；
31. 用户在 Desktop 建卡并指派给某个 Agent，该 Agent 被唤醒并开始处理；
32. daemon 重启后，此前被引擎持有的认领全部释放。

### P5 — 主动性与防死循环

`idle` 轮转 + `agenda` 小脑门控 + `scanner`；停滞推动的三约束；三道防死循环；DM。

33. 房间无新消息时，有未完成卡片的 Agent 被 agenda 唤醒；没有任何可行动项时**不唤醒大脑**，且有记录说明原因；
34. 同一个停滞房间在冷却期内只被推动一次，且只有一个 Agent 去推；
35. 连续 3 次推动被判定无必要后停止推动，房间出现新消息后计数清零；
36. 两个 Agent 的 DM 在第 8 条时触发死循环探测；构造一段无进展来回，探测生效并终止；
37. 由 agenda 唤醒的发言在界面上可辨识，不与普通回复混淆。

### P6 — 收尾

细粒度事件流与日志抽屉；**collab 自己的 GC worker**；三语补齐；文档定稿。

38. 日志抽屉能还原一次唤醒的完整经过：triage 裁决 → prompt 起止 → 工具与命令执行 → 发言 → usage；
39. 连续运行一周，`collab_events` / `collab_triages` 不失控增长；
40. 三份 locale 无缺键（无任何界面文案回退成 key 名）。

### 贯穿全程

41. `openwork-collab` 的依赖图中不出现 `openwork-core`；
42. `crates/openwork-core/migrations/` 无任何 `collab_*` 相关变更；
43. 只使用非 `/experimental/` 的 OpenCode 端点；
44. 前端不出现任何协作语义判定（HELD、去重、认领的裁决），界面只显示 daemon 给的结果；
45. 时间列全部为 `TIMESTAMP WITHOUT TIME ZONE` 东八区，出库字符串带 `+08:00`（见 [.claude/rules/database.md](../.claude/rules/database.md)）。
