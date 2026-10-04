# Agent Note: 参考 cumora 与 raft，用 TypeScript 从零实现 Crew

Status: proposed

## 问题

OpenWork 由 Rust workspace（`openwork-collab`、`openwork-sandbox`，约 3.1 万行）与 Tauri 2 桌面应用组成。项目是求职作品，目标岗位是全栈与 AI 应用工程师。

- 前端已经是 TypeScript，Server、Computer 与 shim 是 Rust。一个功能常常跨两种语言，协议类型要在两边各写一份。
- 两个参考项目 raft 与 cumora 都是 TypeScript 全栈加 Electron。它们的做法不能直接搬进 Rust 代码。
- 目标岗位更看重 TypeScript 全栈能力。

产品范围：本机 macOS、多 Agent 协作、消息、群聊、任务看板、提醒与运行观测。以后可能部署到多机或云端，但当前不做云服务和移动端。

## 提议

参考 cumora、raft 与现有 OpenWork，用 TypeScript 从零实现新版本，产品名改为 Crew。三者都只作参考：

- 不逐行翻译 Rust 代码，不兼容现有的线协议与数据库。
- 现有协作规则（triage 判定、HELD、water-fill 分批等）是做到对应功能时讨论的参考方案之一，不是必须照搬的行为。Rust 版的决策记录在 `.agents/notes/legacy/`。
- 本文是路线图，只记录总体决策与实现顺序。每一步开始前，为这一步的实现决策另写 proposed Agent Note，按根 [AGENTS.md](../../../../AGENTS.md) 的规则讨论；实现后改为 implemented。

已经确定的决策：全部代码用 TypeScript；Desktop、Server、Computer 三个进程；PostgreSQL；Express 5；Electron；Engine 必须在 Seatbelt 中运行；在 `refactor` 分支开发，新代码与 Rust 代码并存到最后一步。

### 已经实现的决策

第 1、2 步与第 3a 步已经实现。当前的结构见 [architecture.md](../../../../docs/architecture.md)，各项决策与理由见：

- [workspace、包划分与构建](../../implemented/architecture/2026-10-04-workspace-and-build.md)
- [主进程监管 Server 与 Computer](../../implemented/architecture/2026-10-04-process-supervision.md)
- [界面直接连接 Server](../../implemented/architecture/2026-10-04-renderer-connects-to-server.md)
- [Express 5 与 protocol 的接口契约](../../implemented/architecture/2026-10-04-express-api-contract.md)
- [私聊的数据模型](../../implemented/architecture/2026-10-04-direct-chat-data-model.md)
- [SSE 只传失效提示与 Agent 唤醒](../../implemented/architecture/2026-10-04-sse-invalidation-and-wake.md)
- [每轮一次 OpenCode 与 crew 命令](../../implemented/architecture/2026-10-04-opencode-turns-and-crew-cli.md)
- [私聊界面](../../implemented/feature/2026-10-04-direct-chat-ui.md)
- [仓库规则、检查与测试流程](../../implemented/process/2026-10-04-repo-rules-and-checks.md)
- [群聊（第 3a 步）](../../implemented/feature/2026-10-05-group-chat.md)
- [会话列表、未读数与侧栏导航](../../implemented/feature/2026-10-05-conversation-list.md)

### 进程与通信

- 三个进程：Electron 主进程启动并监管 Server 与 Computer；界面与 Computer 经 loopback HTTP 与 SSE 访问 Server；Agent 经 `crew` 命令调用 Server。
- Server 与 Computer 只经 HTTP 通信。以后 Server 迁到云端时，Computer 留在用户机器上。

### 技术选型

| 用途 | 选择 |
|---|---|
| HTTP 框架 | Express 5。接口契约在 `packages/protocol/src/api.ts` |
| 协议校验 | zod |
| 数据库访问 | drizzle-orm，驱动用 `pg` |
| 测试 | vitest |
| lint 与格式化 | Biome，两者都开启 |
| 前端 | React、Vite、Tailwind；组件用 shadcn/ui 的做法（Radix 原语加 cva 与 tailwind-merge，组件源码放在仓库里） |
| 界面数据 | TanStack Query |
| SSE 解析 | eventsource-parser |
| `crew` 的参数解析 | commander |
| Markdown | react-markdown 与 remark-gfm；代码高亮用 rehype-highlight |
| Electron 构建与开发 | electron-vite |
| 安装包 | electron-builder，在第 7 步建立 |

### 存储

- PostgreSQL 保存业务事实，是必需依赖：Server 启动时连不上就报错退出，不降级。
- 不用 Redis。只有一个 Server 进程，会过期的协调数据（Agent 状态、凭证）放在 Server 内存里，需要持久的协调数据（例如 HELD 的草稿）放在 PostgreSQL。Server 改为多实例部署时再引入 Redis，届时把进程内事件与内存状态移过去。
- 表结构随功能逐步设计，用 drizzle schema 定义。迁移由 `drizzle-kit generate` 生成并提交，Server 启动时执行。表名不加前缀。
- 事务里不 await 网络调用或 Engine 调用。

### 写操作分层

初步建议每个写操作分三步：

1. 判断：纯函数。输入已加载的状态，输出允许、拒绝或要写入的内容。不访问数据库。
2. 落库：一个事务。加锁，加载状态，调用判断，写入。
3. 通知：事务提交后尽力发布进程内事件与 SSE 失效提示。失败不回滚。

复杂的协调规则写成纯函数，脱离数据库测试。cumora 的 `cumora:server/src/agents/triage-core.ts` 是同样的做法。

### 文档

- 新实现的子系统页随功能编写。Server 与 Computer 的新功能扩展 `docs/subsystems/messaging.md` 与 `docs/subsystems/agent-runtime.md`，看板等独立的功能另起一页。
- `collaboration.md` 与 `collaboration-desktop.md` 描述 Rust 版，开发期间保留作参考，最后一步删除。

### 实现顺序

每一步开始前讨论这一步的实现决策；每一步结束时应用都能启动和演示：

1. 骨架：workspace、Electron、主进程启动 Server 与 Computer、界面显示连接状态。
2. 最小私聊：用户给一个 Agent 发消息，Server 保存消息并唤醒 Agent，Computer 在 Seatbelt 中启动 OpenCode，Agent 经 `crew` 回复，界面显示回复。每个环节只做最简单的版本：一对一私聊，没有 triage、退避与幂等。
3. 群聊协调，分两半：
   - 3a：多个 Agent 的房间、`@handle`、房间成员全部唤醒并由提示词约束何时发言、HELD（发送时有未读的新消息就存为草稿并返回新消息）、界面按序号增量拉取消息。
   - 3b：triage。先用 3a 的版本跑一组对话，统计无关唤醒浪费的 Turn，作为评测集；再用一次便宜的模型调用判断“这条和我有关吗”，用评测集衡量省下的 Turn 与误判，并给 triage 题面加快照。
4. 运行观测：Agent 运行时实时显示它在思考、调用了哪个工具，可以查看每轮的记录。数据来自解析 OpenCode 的事件流。
5. 任务：房间里的消息可以转成任务；状态固定为待办、进行中、待审、完成、关闭，按状态显示成看板或列表；Agent 领取任务是一次 compare-and-swap。分配任务时发一条系统消息并 @ 对方，复用第 3 步的唤醒与 HELD，不另做卡片唤醒。
6. 提醒、记忆与静音：Agent 用 `crew` 给自己定时或周期提醒，到时唤醒它自己；Agent 目录里有 `MEMORY.md`，由 Agent 自己维护；用户可以静音房间。
7. 打包：见下文“打包”。
8. 删除 Rust：删除 `crates/`、Tauri、旧 `desktop/`、描述 Rust 版的文档与 `.agents/notes/legacy/`，卸载 rust-analyzer 相关工具，更新 README、testing.md 与本 Agent Note。

第 3 至 6 步的形状参考了 raft：raft 不做 triage，房间成员全部收到消息（`raft:packages/server/src/services/messageService.ts` 的 `broadcastAndDeliver`），由提示词约束何时插话（`raft:packages/daemon/src/drivers/raftCliGuide.ts` 的 Conversation etiquette），靠发送时的新鲜度检查防止过时的回复（`raft:packages/server/src/routes/internalAgentApi.ts`，最多返回 3 条新消息）；任务状态固定（`raft:packages/server/src/db/schema.ts` 的 `tasks`），分配任务时写一条 “📌 Assigned” 系统消息并 @ 对方（`raft:packages/server/src/services/taskService.ts`）；定时唤醒用 Agent 自己设的提醒，记忆用 `MEMORY.md`（`raft:packages/daemon/src/workspaces.ts`）。

功能对齐后接入新的 Engine 时，写一份“新增 Engine adapter”的操作指南，放在本仓库的 docs/cookbook/ 目录（还没有建）。做法来自 DSH 的 `dsh:docs/cookbook/adding-an-llm-adapter.md`。

界面在对应步骤设计，不复制现有的 `desktop/src`。

### 打包

第 7 步建立安装包：

- 用 electron-builder。Server、Computer、`crew` 与迁移文件经 `extraResources` 放在 asar 之外，做法来自 raft 把 CLI 放在 `Resources/cli/index.js`（`raft:apps/raft-desktop-electron/electron-builder.yml`）。
- 界面经自定义协议 `app://crew` 加载，CORS 来源随之改为它。raft 与 cumora 都用 `app://`。
- 保留 Electron 的 `runAsNode` fuse：Server、Computer 与 `crew` 都靠它运行。
- 打包后的配置来源届时再定。

## 考虑过的方案

**用内嵌 SQLite 替代 PostgreSQL。** 单用户、单写者时 SQLite 足够，也不需要 Docker。没有采用，因为以后可能部署到多机或云端，PostgreSQL 是之后最难替换的部分。

**本机默认用 PGlite，配置 `DATABASE_URL` 时用 PostgreSQL。** PGlite 是编译成 WASM 的 PostgreSQL，raft 用它跑测试（`raft:packages/server/src/db/index.ts`）。它免去 Docker，SQL 方言不变。没有采用，选择统一使用真实 PostgreSQL。

**Redis 作为必需依赖。** 原计划 PostgreSQL 保存业务事实，Redis 保存可过期的协调数据，两者都必需，理由是以后 Server 多实例部署时协调数据与事件要跨实例共享。第 2 步结束时 Redis 只在启动时检查连接。读 raft 后放弃：raft 的 Redis 是可选的，没有它时按单实例运行，Redis 只用于多副本之间的同步（`raft:packages/server/src/replicaRouter.ts`）；它的 HELD 草稿存在 PostgreSQL（`attested_send_pending_drafts`）。Crew 只有一个 Server 进程，不用 Redis 能少一个服务和一组测试配置。

**第 3 步先做 triage。** 原计划第 3 步一起做 triage、点名路由、HELD、连发与逐字重复检查。改为先做不带 triage 的 3a：triage 是减少无关 Turn 的优化，不是群聊能工作的前提；先测出浪费的 Turn，triage 才有评测数据。连发与逐字重复检查删掉，raft 也没有，观察到实际问题时再加。

**看板沿用旧版：自定义列加 `kind`，卡片有独立的唤醒。** 见 legacy 的 `.agents/notes/legacy/architecture/2026-09-24-column-kind-and-card-claim.md` 与 `.agents/notes/legacy/architecture/2026-09-24-persistent-card-wakes.md`。没有采用：固定状态足够演示，分配任务复用消息与 @ 唤醒，不需要第二套唤醒机制。

**Agenda：系统定期用模型判断 Agent 该不该主动做事。** 旧版的做法，有候选集、签名、decline 计数与退避（`docs/subsystems/collaboration.md` §12）。没有采用：换成 Agent 自己设的提醒，更简单，行为可以预期，也容易演示。

**Climate：Agent 对其他参与者的私有印象。** 旧版的做法（`docs/subsystems/collaboration.md` §10）。没有采用：换成 Agent 自己维护的 `MEMORY.md`，不需要专门的表与命令。

**Server 与 Computer 合并为一个进程，或全部放进 Electron 主进程。** 进程更少。没有采用，因为以后 Computer 要留在用户机器上，而 Server 可能迁到云端。全部放进主进程时，Engine 管理、数据库写入与界面通信无法隔离。

**Computer 守护进程用 Go。** 单个二进制便于分发到其他机器。没有采用：资源大头是 Engine 进程，守护进程的语言对内存与延迟影响很小。分发真正成为问题时，再按 HTTP 协议用 Go 重写它。

**守护进程用 Rust，Server 用 Go，只有前端用 TypeScript。** 一个功能要改两到三种语言，协议类型要在三种语言间同步，还要维护三套工具链。性能收益落不到用户可感知的地方，也与转向 TypeScript 的初衷相反。

**保留 Tauri。** Tauri 使用系统 WebView，内存占用低于 Electron。没有采用，因为两个参考项目都用 Electron，而且 Electron 自带的 Node 运行时可以直接运行 `crew` 命令。

**新建仓库。** 没有采用。在当前分支开发可以保留 git 历史与 `.agents/notes/` 中的决策记录。

**按进程逐个替换 Rust 代码。** 先用 Electron 与新界面替换 Tauri，拉起现有的 Rust Server 与 Computer（`openwork-collab server|computer`）；再依次用 TypeScript 替换 Computer 与 Server。每个阶段应用都完整可用。没有采用：TypeScript 一侧在替换完成前必须兼容 Rust 的线协议，不能从零设计接口；而目标是参考三个项目做一个自己的实现。

**先做人与人的聊天，再接 Agent。** 没有采用。人与人的聊天不是产品的核心，按这个顺序要到第三步才能演示 Agent 回复。最小私聊让第二步就能演示核心效果。

**第 2 步建立安装包。** 原计划用打包产物做 `crew` 的冒烟测试。没有采用：冒烟测试用 `electron-vite build` 的产物就能覆盖 `crew` 在沙箱中的运行；“asar 内的 `crew` 能否读取”由打包时放在 asar 之外解决；安装包带来的自定义协议、打包后的配置与签名问题第 2 步用不上。

## 验收条件

- 第 1 至 7 步每一步结束时，`docker compose up -d` 与 `pnpm dev` 能启动应用，已完成的功能可以演示，`pnpm check` 通过。
- 第 8 步结束后，仓库中没有 Rust 与 Tauri 代码，文档中没有 Rust 路径。

## 风险

- Electron 的内存占用高于 Tauri。
- 演示前需要安装 Docker 并启动 PostgreSQL。
- 开发期间 Rust 与 TypeScript 两套代码并存。Rust 代码不再加功能，只修影响旧版本运行的问题。
- 从零实现时，现有 Rust 版已经解决的问题可能重新出现。做到对应功能时，先查旧版的子系统页与 `.agents/notes/legacy/`。
