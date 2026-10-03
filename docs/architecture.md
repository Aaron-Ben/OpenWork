# 架构

OpenWork 是本地多 Agent 协作工作台。人和多个 Agent 在房间里交流，在看板上分工；每个 Agent 由本机的 Engine 执行，当前 Engine 是 OpenCode。它由 Rust workspace 与 Tauri 2 / React 桌面应用组成，用 PostgreSQL 保存业务事实，用 Redis 传递 wake 与短期协调状态。

本文定义 crate 划分、依赖方向和所有权边界。行为细节见 [collaboration.md](subsystems/collaboration.md) 与 [collaboration-desktop.md](subsystems/collaboration-desktop.md)，索引见 [README](README.md)。

## 1. 依赖方向

```text
desktop/src          ← 只通过 Tauri Command/Event
desktop/src-tauri    → openwork-collab
openwork-collab      → openwork-sandbox
openwork-sandbox     → 无 OpenWork 依赖
```

**这个方向不可逆转。** 具体禁止以下情况：

- `openwork-sandbox` 依赖任何其他 OpenWork crate，或自己启动被限制的进程（启动自检除外）；
- Desktop 直接依赖 SQLx 或 Redis；
- **Desktop 绕过 Collaboration Server，直接读写 `collab_*` 表**。

## 2. 各 crate 的职责

### openwork-collab

协作只用一个 crate，把各自独立的生命周期放进内部模块：

```text
protocol/   ← server/
protocol/   ← computer/
server/     ← bin/openwork-collab.rs
computer/   ← bin/openwork-collab.rs
protocol/   ← bin/openwork.rs
```

- `protocol/` 只保存 Server、Computer、shim 与 Desktop host 共用的线协议。它不包含 SQL、Redis、进程管理、Engine 原生事件或业务判断。
- `server/` 是协作世界持久事实的唯一写者。它拥有 loopback HTTP/SSE、RuntimeSession、房间、消息、看板、Climate、Run、Agenda、PostgreSQL 与 Redis 协调。它不启动 Engine。
- `computer/` 是 macOS 本机 BYOA 宿主。它拥有 desired-state reconcile、`AgentRunner`、home、shim、`EngineRegistry`、`EngineAdapter` 接口与生产 `OpenCodeAdapter`。它经 `openwork-sandbox` 在 Seatbelt 下启动 Engine。它不使用 SQLx 或 Redis，也不持有 Server 的数据库凭证。

进程与通信边界：

```text
React → Tauri Command → /desktop/* HTTP/SSE → Collaboration Server
Computer daemon → /computer/* HTTP/SSE → Collaboration Server
Agent shim → /agent/* HTTP → Collaboration Server
Collaboration Server → PostgreSQL + Redis
Computer daemon → per-Agent Engine child processes
```

Desktop 是唯一的 supervisor。每次启动时，它先创建一个 RuntimeSession 和临时凭证，再启动 Server 与 Computer。Desktop 退出时，先停止 Computer 及其 Engine 进程组，再停止 Server。Runtime 不使用系统常驻任务、持久机器身份、固定端口或本地 socket transport。

单 crate 是有意的选择。当前实现只随同一个 macOS 应用构建、安装和升级。protocol 没有独立的发布者，Computer 也没有远程部署目标。只为目录边界拆 crate，会增加 manifest、错误类型、测试 fixture 与版本协商，但不会增加实际隔离。

### openwork-sandbox

**回答“这个进程能读写哪里”，并让内核兑现它。** 协作用它的 `EngineConfinement` 把每个 Engine 进程关进 Seatbelt：可写根、`$HOME` 下的可读例外与对应的 Seatbelt profile（[collaboration.md §3.1](subsystems/collaboration.md)）。围栏里放哪些目录，由 Computer 按 Agent home 布局决定。公开接口见 [crate README](../crates/openwork-sandbox/README.md)。

### desktop

`desktop/src-tauri` 监督 Server 与 Computer 两个进程，只经 Desktop HTTP 接口访问协作。它把 Server 的 SSE 失效通知转成 Tauri Event。`desktop/src` 是 React 界面：房间、Agent 私聊、同事、看板、运行观测与设置。

## 3. 核心不变量

1. **Server 是业务事实的唯一写者。** Computer、shim 与 Desktop 只经 HTTP 接口读写。
2. **Engine 推进推理。** `AgentRunner` 只交付 wake delta 并记录结果，不自己调用模型，见 [collaboration.md](subsystems/collaboration.md)。
3. **Provider 登录态只在 Engine 自己的 data root 中。** Server 不持有 Provider 凭证；主推理与 triage 都用 Computer 上 OpenCode 的登录态。
4. **持久数据、短期协调、瞬时状态分开存放**：持久数据在 `collab_*` 表，短期协调在 Redis，瞬时 Runner 状态在 Computer 内存。

## 4. 领域词汇

- **Room**：人和 Agent 交流的房间，分为私聊与群聊。
- **协作 Agent**：有显式 `engine_id`、主模型、triage 模型、私有 home、当前 RuntimeSession JWT 与 `AgentRunner` 的同事。
- **Engine**：执行 Agent 工作的本机程序。当前只有 `OpenCodeAdapter`。
- **Board / Card**：看板与卡片，Agent 可以认领卡片。
- **Agenda**：Agent 主动工作的计划。
- **Run**：Agent 一次被唤醒后的执行记录，供运行观测查看。
- **RuntimeSession**：Desktop 每次启动创建的运行期身份，进程与凭证都绑定在它上面。

## 5. 当前非目标

协作范围严格限定为本机 macOS、消息、群聊、Climate、看板和 Agenda，当前 Engine 只有 OpenCode。范围不含：Windows/Linux、远程 Mac、多 Computer assignment、系统后台常驻、MCP、审批、共享项目目录、Git/Diff、Worktree、Memory 或 reaction。

未来的 Codex adapter 属于已经确认的 Engine seam，但只在真实接入时实现，不预留空实现或 capability 矩阵。若有人重新提出其他范围，先重新评审。不要因此让 Computer 直连数据库、让 Server 启动 Engine，或让 Desktop 承担业务规则。

## 6. 新增模块时的检查

每引入一个模块，回答六个问题：

1. 谁创建它；
2. 谁修改它；
3. 谁决定下一状态；
4. 谁持久化；
5. 它失败是否会改变主流程；
6. 是否产生反向依赖。

**若一个类型需要同时回答两个以上的 Owner，先拆职责再落地。**
