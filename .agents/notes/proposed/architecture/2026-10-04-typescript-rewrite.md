# Agent Note: 参考 cumora 与 raft，用 TypeScript 从零实现 OpenWork

Status: proposed

## 问题

OpenWork 由 Rust workspace（`openwork-collab`、`openwork-sandbox`，约 3.1 万行）与 Tauri 2 桌面应用组成。项目是求职作品，目标岗位是全栈与 AI 应用工程师。

- 前端已经是 TypeScript，Server、Computer 与 shim 是 Rust。一个功能常常跨两种语言，协议类型要在两边各写一份。
- 两个参考项目 raft 与 cumora 都是 TypeScript 全栈加 Electron。它们的做法不能直接搬进 Rust 代码。
- 目标岗位更看重 TypeScript 全栈能力。

产品范围不变（[architecture.md §5](../../../../docs/architecture.md)）：本机 macOS、多 Agent 协作、消息、群聊、Climate、看板与 Agenda。以后可能部署到多机或云端，但当前不做云服务和移动端。

## 提议

参考 cumora、raft 与现有 OpenWork，用 TypeScript 从零实现一个新版本。三者都只作参考：

- 不逐行翻译 Rust 代码，不兼容现有的线协议与数据库。
- 现有协作规则（triage 判定、HELD、water-fill 分批等）是做到对应功能时讨论的参考方案之一，不是必须照搬的行为。
- 具体实现决策在对应步骤开始前逐个讨论，按根 [AGENTS.md](../../../../AGENTS.md) 的规则进行。下文“进程与通信”“包划分”“Agent 运行”“写操作分层”中的实现细节是初步建议，到对应步骤再定。

已经确定的决策：全部代码用 TypeScript；Desktop、Server、Computer 三个进程；PostgreSQL 与 Redis；Hono；Electron；Engine 必须在 Seatbelt 中运行；在 `refactor` 分支开发，新代码与 Rust 代码并存到最后一步。

### 进程与通信

三个进程，进程之间的边界与现有版本相同：

```text
Electron 主进程 ── 启动 ──→ Server ──→ PostgreSQL + Redis
               └─ 启动 ──→ Computer ──spawn──→ 每个 Agent 一个 Engine 进程（Seatbelt 内）
React 界面     ── loopback HTTP + SSE ──→ Server
Computer       ── loopback HTTP + SSE ──→ Server
Agent shim     ── loopback HTTP ──→ Server
```

- Electron 主进程只做 supervisor：先起 Server，再起 Computer，退出时按相反顺序停止。
- 界面直接连 Server。Server 每次启动使用随机端口与临时凭证，所以地址与凭证在运行时经 preload 交给界面。raft 与 cumora 在构建时写入 API 地址，因为它们连接固定的云端地址，这种做法在这里不成立。
- Server 与 Computer 只经 HTTP 通信。以后 Server 迁到云端时，Computer 留在用户机器上。
- raft 与 cumora 的 Electron 主进程都不启动本地 Server，主进程怎样启动与监管 Server 和 Computer 需要自己设计。现有 Rust 版的 bootstrap/ready 协议（`crates/openwork-collab/src/protocol/desktop.rs`）是参考之一。

### 包划分

```text
packages/protocol/   线协议：zod schema 与推导出的类型
packages/server/     Collaboration Server
packages/computer/   Agent 宿主：Engine adapter、Agent 调度、home、Seatbelt、shim
apps/desktop/        Electron 主进程、preload 与 React 界面
```

- 用 pnpm workspace 管理。
- 沙箱代码只有 Computer 一个使用方，放在 `packages/computer` 内，不单独成包。
- shim 只被 Computer 使用，作为 `packages/computer` 的第二个入口，不单独成包。

### 技术选型

| 用途 | 选择 |
|---|---|
| HTTP 框架 | Hono。界面用 `hono/client` 获得有类型的接口客户端 |
| 协议校验 | zod |
| 数据库访问 | drizzle-orm，驱动用 `pg` |
| Redis | ioredis |
| 测试 | vitest |
| lint 与格式化 | Biome，两者都开启 |
| 前端 | React、Vite、zustand、Tailwind |

Electron 主进程的构建与开发方式、安装包工具，在第 1 步讨论决定。

### 存储

- PostgreSQL 保存业务事实，Redis 保存可过期、可重建的协调数据。
- 两者都是必需依赖。Server 启动时连不上就报错退出，不降级。
- `compose.yaml` 同时定义 `postgres` 与 `redis`。`docker compose up -d` 起全部依赖。
- 表结构随功能逐步设计，用 drizzle schema 定义。迁移由 `drizzle-kit generate` 生成并提交，Server 启动时执行。表名不加前缀。
- Redis 读写集中在 Server 的一个模块里。
- 事务里不 await 网络调用或 Engine 调用。

### 写操作分层

初步建议每个写操作分三步：

1. 判断：纯函数。输入已加载的状态，输出允许、拒绝或要写入的内容。不访问数据库和 Redis。
2. 落库：一个事务。加锁，加载状态，调用判断，写入。
3. 通知：事务提交后尽力发布 Redis 事件与 SSE 失效提示。失败不回滚。

复杂的协调规则写成纯函数，脱离数据库测试。cumora 的 `server/src/agents/triage-core.ts` 是同样的做法。

### Agent 运行

初步建议：

- Engine adapter 参考现有 Rust 版的两层结构（adapter 与每个 Agent 的 runtime）、cumora 的 `server/src/agents/computer/engine.ts` 与 raft 的 `packages/daemon/src/drivers/`。
- 取消用 `AbortSignal`。Engine 以独立进程组启动，取消时先发 SIGINT，再发 SIGKILL。
- Seatbelt profile 由 TypeScript 生成，经 `/usr/bin/sandbox-exec -p` 启动 Engine。没有无沙箱的运行路径。
- shim 用 Electron 自带的可执行文件运行：以 `ELECTRON_RUN_AS_NODE=1` 执行 Electron 可执行文件与打包后的 `shim.js`。两者都在 `$HOME` 之外，沙箱可以读取。
- 第一版只接 OpenCode。

### 文档

- 新实现的子系统页随功能编写，放在 `docs/subsystems/`。怎样拆分页面，做到对应功能时决定。
- 现有的 `collaboration.md` 与 `collaboration-desktop.md` 描述 Rust 版，开发期间保留作参考，最后一步删除。

### 仓库规则文件

做法来自 DSH（`/Volumes/Extreme SSD/Code/deepseek-harness`）：

- 根目录的规则文件是 `AGENTS.md`，`CLAUDE.md` 是指向它的软链接。Claude Code 与 Codex 读到同一份规则。DSH 的根 `CLAUDE.md` 就是指向 `AGENTS.md` 的软链接。
- 根 `AGENTS.md` 只放每个会话都需要的常驻规则。每条一到三行，并链接到理由。
- 新增 `packages/AGENTS.md`，只放包内的代码约定。`docs/AGENTS.md` 继续只放文档规则。做法来自 DSH 的 `packages/AGENTS.md`。

### 代码约定

以下约定写进 `packages/AGENTS.md`，来自 DSH 的根 `AGENTS.md` 与 `packages/AGENTS.md`：

- 跨进程或跨包传递的 ID（`AgentId`、`RoomId`、`RunId` 等）用 branded 类型，不用裸 `string`。
- 同一进程内信任 TypeScript 类型。只在 HTTP、文件、子进程输出、环境变量与配置处用 zod 校验。
- 可辨识联合用 `switch` 处理，封闭的联合以 `assertNever` 结尾。
- 只在操作成功后发通知、更新派生状态。
- 配置缺失或错误时，在启动时报错，不跳过。
- 禁止 `as unknown`。空的 `catch` 写明吞掉的错误和原因。

### 自动检查

能机械检查的规则，写成会被执行的检查脚本。DSH 用 `scripts/verify-*.ts` 检查文档规则，cumora 用 `scripts/guard-*.mjs` 检查跨文件一致性。第一批只有三个，放在 `scripts/`，由 `pnpm lint` 运行：

- Markdown 相对链接指向存在的文件。
- Agent Note 的前三行格式正确，状态行与所在目录一致。
- 根 `AGENTS.md` 不超过字数上限。

git hooks 用 lefthook，只做快速检查，做法来自 DSH 的 `lefthook.yml`：

- pre-commit：对暂存文件运行 Biome（自动修复并重新暂存），运行 `git diff --cached --check`。
- pre-push：运行 `pnpm typecheck`。

### 测试与检查

| 层 | 内容 | 依赖 |
|---|---|---|
| 单元 | 纯判断函数、前端纯函数 | 无 |
| 集成 | Server 的 HTTP 与事务，Computer 与 Server 的协议往返 | `TEST_DATABASE_URL`、`TEST_REDIS_URL` |
| 冒烟 | 起 Server 与 Computer，用脚本化的假 Engine 发一条消息，断言回复落库 | 同上 |

- 每个集成测试文件使用独立的临时数据库，Redis key 加测试专属前缀。
- 假 Engine 是一个正式的 Engine adapter 实现。
- 真实 OpenCode 与 Seatbelt 测试只在 macOS 上手动运行。
- 冒烟测试运行打包后的产物，不运行源码。shim 的冒烟测试用 Electron 可执行文件与打包后的 `shim.js`，在 Seatbelt 中启动。做法来自 DSH `docs/testing.md` 的“Test the real entry path”。
- 模型可见的文本（每轮 Turn 的输入、triage 题面、shim 的输出）用 vitest 文件快照逐字锁定。改动这些文本时，快照的 diff 进入代码审查。
- 日常检查：`pnpm lint`、`pnpm typecheck`。提交或合并前：`pnpm check`。
- 提交前按改动选择能覆盖它的最小测试集，只汇报实际运行的命令。做法来自 DSH 的 `.agents/skills/dsh-pre-push-checks/SKILL.md`，写成本仓库的 `.agents/skills/` 中的一个 skill。
- 本地开发：`docker compose up -d` 后运行 `pnpm dev`。

### 实现顺序

每一步开始前讨论这一步的实现决策；每一步结束时应用都能启动和演示：

1. 骨架：workspace、protocol、Server、Electron supervisor，界面能连上 Server。同一步建立 `packages/AGENTS.md`、Biome、lefthook 与三个检查脚本，并在根 `AGENTS.md` 的“运行与检查”中加入 TypeScript 的命令。
2. 最小私聊：用户给一个 Agent 发消息，Server 保存消息并唤醒 Agent，Computer 在 Seatbelt 中启动 OpenCode，Agent 经 shim 回复，界面显示回复。每个环节只做最简单的版本：一个房间，没有 triage、退避与幂等。同一步加入 shim 的打包产物冒烟测试、Turn 输入的快照与提交前检查的 skill。写 Computer 时，按 DSH `docs/defensive-patterns.md` 检查子进程与清理代码；本仓库的 `docs/defensive-patterns.md` 只记录实际出现过的缺陷。
3. 群聊协调：多个 Agent、triage、点名路由、HELD、连发与逐字重复，以及 triage 题面的快照。
4. 看板：Board、Column、Card、领取与卡片唤醒。
5. 其余功能：Agenda、Climate、静音、运行观测。
6. 删除 Rust：删除 `crates/`、Tauri、旧 `desktop/`、`scripts/check.sh` 与描述 Rust 版的文档，卸载 rust-analyzer 相关工具，更新 README、architecture.md、testing.md 与本 Agent Note。

功能对齐后接入新的 Engine 时，写一份“新增 Engine adapter”的操作指南，放在 `docs/cookbook/`。做法来自 DSH 的 `docs/cookbook/adding-an-llm-adapter.md`。

新代码放在根目录的 `packages/` 与 `apps/`。界面在对应步骤设计，不复制现有的 `desktop/src`。

## 考虑过的方案

**用内嵌 SQLite 替代 PostgreSQL。** 单用户、单写者时 SQLite 足够，也不需要 Docker。没有采用，因为以后可能部署到多机或云端，PostgreSQL 是之后最难替换的部分。

**本机默认用 PGlite，配置 `DATABASE_URL` 时用 PostgreSQL。** PGlite 是编译成 WASM 的 PostgreSQL，raft 用它跑测试（`packages/server/src/db/index.ts`）。它免去 Docker，SQL 方言不变。没有采用，选择统一使用真实 PostgreSQL。

**用 Server 进程内状态替代 Redis。** Node 单线程事件循环下，无需加锁就能原子地完成 claim。没有采用，因为以后 Server 多实例部署时，wake 与事件需要跨实例传递。

**Server 与 Computer 合并为一个进程，或全部放进 Electron 主进程。** 进程更少。没有采用，因为以后 Computer 要留在用户机器上，而 Server 可能迁到云端。全部放进主进程时，Engine 管理、数据库写入与界面通信无法隔离。

**Computer 守护进程用 Go。** 单个二进制便于分发到其他机器。没有采用：资源大头是 Engine 进程，守护进程的语言对内存与延迟影响很小。分发真正成为问题时，再按 HTTP 协议用 Go 重写它。

**守护进程用 Rust，Server 用 Go，只有前端用 TypeScript。** 一个功能要改两到三种语言，协议类型要在三种语言间同步，还要维护三套工具链。性能收益落不到用户可感知的地方，也与转向 TypeScript 的初衷相反。

**HTTP 框架用 Express。** raft 与 cumora 都用 Express（各自的 `package.json`）。没有采用，因为 Hono 能让 Server 的路由类型直接约束前端调用。

**保留 Tauri。** Tauri 使用系统 WebView，内存占用低于 Electron。没有采用，因为两个参考项目都用 Electron，而且 Electron 自带的 Node 运行时可以直接运行 shim。

**新建仓库。** 没有采用。在当前分支开发可以保留 git 历史与 `.agents/notes/` 中的决策记录。

**照搬 DSH 的完整文档与检查体系。** DSH 有 71 个 `scripts/verify-*` 检查、中英双语文档（`.md`、`.zh.md`、`.i18n.yaml`）、冻结的 Agent Note 归档、每个文件 100% 的覆盖率门槛和录制会话回放的快照。没有采用，因为这些服务于多人协作的大型仓库，维护成本超过单人项目的收益。只取三个检查脚本、git hooks 与代码约定。

**按进程逐个替换 Rust 代码。** 先用 Electron 与新界面替换 Tauri，拉起现有的 Rust Server 与 Computer（`openwork-collab server|computer`）；再依次用 TypeScript 替换 Computer 与 Server。每个阶段应用都完整可用。没有采用：TypeScript 一侧在替换完成前必须兼容 Rust 的线协议，不能从零设计接口；而目标是参考三个项目做一个自己的实现。

**先做人与人的聊天，再接 Agent。** 没有采用。人与人的聊天不是产品的核心，按这个顺序要到第三步才能演示 Agent 回复。最小私聊让第二步就能演示核心效果。

## 验收条件

- 第 1 至 5 步每一步结束时，`docker compose up -d` 与 `pnpm dev` 能启动应用，已完成的功能可以演示。
- 第 2 步结束时，用户在私聊中给 Agent 发消息，Agent 在 Seatbelt 中运行 OpenCode，并经 shim 回复。
- Seatbelt 不可用时，Computer 不启动任何 Agent。
- 修改一个 Server 路由的响应字段后，`pnpm typecheck` 在使用该字段的前端代码处报错。
- `pnpm check` 通过：lint、类型检查、单元、集成与冒烟测试。
- 三个检查脚本各有一个测试，证明它会拒绝违规输入：失效的链接、状态行与目录不一致的 Agent Note、超出字数上限的根 `AGENTS.md`。
- `CLAUDE.md` 是指向 `AGENTS.md` 的软链接。
- 打包后的 shim 在 Seatbelt 中能调用 Server，由冒烟测试覆盖。
- 第 6 步结束后，仓库中没有 Rust 与 Tauri 代码，文档中没有 Rust 路径。

## 风险

- shim 依赖 Electron 的 `runAsNode` fuse。关闭这个 fuse 的打包配置会让 shim 无法启动。shim 在 Seatbelt 中经 `ELECTRON_RUN_AS_NODE` 启动的行为与启动耗时都没有实测。
- Electron 的内存占用高于 Tauri。
- 演示前需要安装 Docker 并启动 PostgreSQL 与 Redis。
- 开发期间 Rust 与 TypeScript 两套代码并存。Rust 代码不再加功能，只修影响旧版本运行的问题。
- Hono 的生态小于 Express，参考项目中的 Express 路由写法需要转换。
- 从零实现时，现有 Rust 版已经解决的问题可能重新出现。做到对应功能时，先查现有的子系统页与 Agent Note。
