# Agent Note: 参考 cumora 与 raft，用 TypeScript 从零实现 Crew

Status: proposed

## 问题

OpenWork 由 Rust workspace（`openwork-collab`、`openwork-sandbox`，约 3.1 万行）与 Tauri 2 桌面应用组成。项目是求职作品，目标岗位是全栈与 AI 应用工程师。

- 前端已经是 TypeScript，Server、Computer 与 shim 是 Rust。一个功能常常跨两种语言，协议类型要在两边各写一份。
- 两个参考项目 raft 与 cumora 都是 TypeScript 全栈加 Electron。它们的做法不能直接搬进 Rust 代码。
- 目标岗位更看重 TypeScript 全栈能力。

产品范围不变：本机 macOS、多 Agent 协作、消息、群聊、Climate、看板与 Agenda。以后可能部署到多机或云端，但当前不做云服务和移动端。

## 提议

参考 cumora、raft 与现有 OpenWork，用 TypeScript 从零实现新版本，产品名改为 Crew。三者都只作参考：

- 不逐行翻译 Rust 代码，不兼容现有的线协议与数据库。
- 现有协作规则（triage 判定、HELD、water-fill 分批等）是做到对应功能时讨论的参考方案之一，不是必须照搬的行为。Rust 版的决策记录在 `.agents/notes/legacy/`。
- 本文是路线图，只记录总体决策与实现顺序。每一步开始前，为这一步的实现决策另写 proposed Agent Note，按根 [AGENTS.md](../../../../AGENTS.md) 的规则讨论；实现后改为 implemented。

已经确定的决策：全部代码用 TypeScript；Desktop、Server、Computer 三个进程；PostgreSQL 与 Redis；Express 5；Electron；Engine 必须在 Seatbelt 中运行；在 `refactor` 分支开发，新代码与 Rust 代码并存到最后一步。

### 已经实现的决策

第 1、2 步已经实现。当前的结构见 [architecture.md](../../../../docs/architecture.md)，各项决策与理由见：

- [workspace、包划分与构建](../../implemented/architecture/2026-10-04-workspace-and-build.md)
- [主进程监管 Server 与 Computer](../../implemented/architecture/2026-10-04-process-supervision.md)
- [界面直接连接 Server](../../implemented/architecture/2026-10-04-renderer-connects-to-server.md)
- [Express 5 与 protocol 的接口契约](../../implemented/architecture/2026-10-04-express-api-contract.md)
- [私聊的数据模型](../../implemented/architecture/2026-10-04-direct-chat-data-model.md)
- [SSE 只传失效提示与 Agent 唤醒](../../implemented/architecture/2026-10-04-sse-invalidation-and-wake.md)
- [每轮一次 OpenCode 与 crew 命令](../../implemented/architecture/2026-10-04-opencode-turns-and-crew-cli.md)
- [私聊界面](../../implemented/feature/2026-10-04-direct-chat-ui.md)
- [仓库规则、检查与测试流程](../../implemented/process/2026-10-04-repo-rules-and-checks.md)

### 进程与通信

- 三个进程：Electron 主进程启动并监管 Server 与 Computer；界面与 Computer 经 loopback HTTP 与 SSE 访问 Server；Agent 经 `crew` 命令调用 Server。
- Server 与 Computer 只经 HTTP 通信。以后 Server 迁到云端时，Computer 留在用户机器上。

### 技术选型

| 用途 | 选择 |
|---|---|
| HTTP 框架 | Express 5。接口契约在 `packages/protocol/src/api.ts` |
| 协议校验 | zod |
| 数据库访问 | drizzle-orm，驱动用 `pg` |
| Redis | ioredis |
| 测试 | vitest |
| lint 与格式化 | Biome，两者都开启 |
| 前端 | React、Vite、Tailwind；组件用 shadcn/ui 的做法（Radix 原语加 cva 与 tailwind-merge，组件源码放在仓库里） |
| 界面数据 | TanStack Query |
| SSE 解析 | eventsource-parser |
| `crew` 的参数解析 | commander |
| Markdown | react-markdown 与 remark-gfm；代码高亮用 rehype-highlight |
| Electron 构建与开发 | electron-vite |
| 安装包 | electron-builder，在第 6 步建立 |

### 存储

- PostgreSQL 保存业务事实，Redis 保存可过期、可重建的协调数据。
- 两者都是必需依赖。Server 启动时连不上就报错退出，不降级。`compose.yaml` 同时定义两者。
- 表结构随功能逐步设计，用 drizzle schema 定义。迁移由 `drizzle-kit generate` 生成并提交，Server 启动时执行。表名不加前缀。
- Redis 读写集中在 Server 的一个模块里。测试使用 Redis 时，key 加测试专属的前缀。
- 事务里不 await 网络调用或 Engine 调用。

### 写操作分层

初步建议每个写操作分三步：

1. 判断：纯函数。输入已加载的状态，输出允许、拒绝或要写入的内容。不访问数据库和 Redis。
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
3. 群聊协调：多个 Agent、triage、点名路由、HELD、连发与逐字重复，以及 triage 题面的快照。需要新增 handle（`@alice`）、triage、投递与运行记录。
4. 看板：Board、Column、Card、领取与卡片唤醒。
5. 其余功能：Agenda、Climate、静音、运行观测。
6. 打包：见下文“打包”。
7. 删除 Rust：删除 `crates/`、Tauri、旧 `desktop/`、描述 Rust 版的文档与 `.agents/notes/legacy/`，卸载 rust-analyzer 相关工具，更新 README、testing.md 与本 Agent Note。

功能对齐后接入新的 Engine 时，写一份“新增 Engine adapter”的操作指南，放在本仓库的 docs/cookbook/ 目录（还没有建）。做法来自 DSH 的 `dsh:docs/cookbook/adding-an-llm-adapter.md`。

界面在对应步骤设计，不复制现有的 `desktop/src`。

### 打包

第 6 步建立安装包：

- 用 electron-builder。Server、Computer、`crew` 与迁移文件经 `extraResources` 放在 asar 之外，做法来自 raft 把 CLI 放在 `Resources/cli/index.js`（`raft:apps/raft-desktop-electron/electron-builder.yml`）。
- 界面经自定义协议 `app://crew` 加载，CORS 来源随之改为它。raft 与 cumora 都用 `app://`。
- 保留 Electron 的 `runAsNode` fuse：Server、Computer 与 `crew` 都靠它运行。
- 打包后的配置来源届时再定。

## 考虑过的方案

**用内嵌 SQLite 替代 PostgreSQL。** 单用户、单写者时 SQLite 足够，也不需要 Docker。没有采用，因为以后可能部署到多机或云端，PostgreSQL 是之后最难替换的部分。

**本机默认用 PGlite，配置 `DATABASE_URL` 时用 PostgreSQL。** PGlite 是编译成 WASM 的 PostgreSQL，raft 用它跑测试（`raft:packages/server/src/db/index.ts`）。它免去 Docker，SQL 方言不变。没有采用，选择统一使用真实 PostgreSQL。

**用 Server 进程内状态替代 Redis。** Node 单线程事件循环下，无需加锁就能原子地完成 claim。没有采用，因为以后 Server 多实例部署时，协调数据与事件需要跨实例共享。

**Server 与 Computer 合并为一个进程，或全部放进 Electron 主进程。** 进程更少。没有采用，因为以后 Computer 要留在用户机器上，而 Server 可能迁到云端。全部放进主进程时，Engine 管理、数据库写入与界面通信无法隔离。

**Computer 守护进程用 Go。** 单个二进制便于分发到其他机器。没有采用：资源大头是 Engine 进程，守护进程的语言对内存与延迟影响很小。分发真正成为问题时，再按 HTTP 协议用 Go 重写它。

**守护进程用 Rust，Server 用 Go，只有前端用 TypeScript。** 一个功能要改两到三种语言，协议类型要在三种语言间同步，还要维护三套工具链。性能收益落不到用户可感知的地方，也与转向 TypeScript 的初衷相反。

**保留 Tauri。** Tauri 使用系统 WebView，内存占用低于 Electron。没有采用，因为两个参考项目都用 Electron，而且 Electron 自带的 Node 运行时可以直接运行 `crew` 命令。

**新建仓库。** 没有采用。在当前分支开发可以保留 git 历史与 `.agents/notes/` 中的决策记录。

**按进程逐个替换 Rust 代码。** 先用 Electron 与新界面替换 Tauri，拉起现有的 Rust Server 与 Computer（`openwork-collab server|computer`）；再依次用 TypeScript 替换 Computer 与 Server。每个阶段应用都完整可用。没有采用：TypeScript 一侧在替换完成前必须兼容 Rust 的线协议，不能从零设计接口；而目标是参考三个项目做一个自己的实现。

**先做人与人的聊天，再接 Agent。** 没有采用。人与人的聊天不是产品的核心，按这个顺序要到第三步才能演示 Agent 回复。最小私聊让第二步就能演示核心效果。

**第 2 步建立安装包。** 原计划用打包产物做 `crew` 的冒烟测试。没有采用：冒烟测试用 `electron-vite build` 的产物就能覆盖 `crew` 在沙箱中的运行；“asar 内的 `crew` 能否读取”由打包时放在 asar 之外解决；安装包带来的自定义协议、打包后的配置与签名问题第 2 步用不上。

## 验收条件

- 第 1 至 6 步每一步结束时，`docker compose up -d` 与 `pnpm dev` 能启动应用，已完成的功能可以演示，`pnpm check` 通过。
- 第 7 步结束后，仓库中没有 Rust 与 Tauri 代码，文档中没有 Rust 路径。

## 风险

- Electron 的内存占用高于 Tauri。
- 演示前需要安装 Docker 并启动 PostgreSQL 与 Redis。
- 开发期间 Rust 与 TypeScript 两套代码并存。Rust 代码不再加功能，只修影响旧版本运行的问题。
- 从零实现时，现有 Rust 版已经解决的问题可能重新出现。做到对应功能时，先查旧版的子系统页与 `.agents/notes/legacy/`。
