# Agent Note: 用 TypeScript 重写 OpenWork

Status: proposed

## 问题

OpenWork 由 Rust workspace（`openwork-collab`、`openwork-sandbox`，约 3.1 万行）与 Tauri 2 桌面应用组成。项目是求职作品，目标岗位是全栈与 AI 应用工程师。

- 前端已经是 TypeScript，Server、Computer 与 shim 是 Rust。一个功能常常跨两种语言，协议类型要在两边各写一份。
- 两个参考项目 raft 与 cumora 都是 TypeScript 全栈加 Electron。它们的做法不能直接搬进 Rust 代码。
- 目标岗位更看重 TypeScript 全栈能力。

产品范围不变（[architecture.md §5](../../../../docs/architecture.md)）：本机 macOS、多 Agent 协作、消息、群聊、Climate、看板与 Agenda。以后可能部署到多机或云端，但当前不做云服务和移动端。

## 提议

全部代码改用 TypeScript。现有 Rust 代码只作为行为参考，不逐行翻译，不兼容旧数据库。重写在 `refactor` 分支进行。

### 进程与通信

保留现有的三个进程和它们之间的边界：

```text
Electron 主进程 ── utilityProcess ──→ Server ──→ PostgreSQL + Redis
               └─ utilityProcess ──→ Computer ──spawn──→ 每个 Agent 一个 Engine 进程（Seatbelt 内）
React 界面     ── loopback HTTP + SSE ──→ Server
Computer       ── loopback HTTP + SSE ──→ Server
Agent shim     ── loopback HTTP ──→ Server
```

- Electron 主进程只做 supervisor。它沿用 RuntimeSession：先起 Server，再起 Computer，退出时按相反顺序停止。
- 界面直接连 Server。preload 把 Server 地址和 Desktop 凭证交给界面，不再经过 Tauri Command 转发。
- SSE 只传失效提示，正确性靠 PostgreSQL（[Agent Note](../../implemented/architecture/2026-09-01-sse-invalidation-only.md)）。
- Server 与 Computer 只经 HTTP 通信。以后 Server 迁到云端时，Computer 留在用户机器上，协议不变。

### 包划分

```text
packages/protocol/   线协议：zod schema 与推导出的类型
packages/server/     Collaboration Server
packages/computer/   Agent 宿主：EngineAdapter、AgentRunner、home、Seatbelt、shim
apps/desktop/        Electron 主进程、preload 与 React 界面
```

- 用 pnpm workspace 管理。
- `openwork-sandbox` 只有 Computer 一个使用方，并入 `packages/computer/src/sandbox/`。
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
| 打包 | server 与 computer 用 tsup；界面与 Electron 用 electron-vite；安装包用 electron-builder |
| 前端 | 沿用现有的 React 19、Vite、zustand、Tailwind 4 |

### 存储

- PostgreSQL 保存业务事实，Redis 保存可过期、可重建的协调数据。分工与 [collaboration.md §13](../../../../docs/subsystems/collaboration.md) 相同。
- 两者都是必需依赖。Server 启动时连不上就报错退出，不降级。
- `compose.yaml` 同时定义 `postgres` 与 `redis`。`docker compose up -d` 起全部依赖。
- 现有 17 张表用 drizzle schema 重新定义，合成一份初始迁移，去掉 `collab_` 前缀。迁移由 `drizzle-kit generate` 生成并提交，Server 启动时执行。
- Redis 读写全部集中在 Server 的 `coordination` 模块。
- 事务里不 await 网络调用或 Engine 调用。

### 写操作分层

每个写操作分三步：

1. 判断：纯函数。输入已加载的状态，输出允许、拒绝或要写入的内容。不访问数据库和 Redis。
2. 落库：一个事务。按固定顺序加锁，加载状态，调用判断，写入，同一事务写命令幂等结果。
3. 通知：事务提交后尽力发布 Redis 事件与 SSE 失效提示。失败不回滚。

triage 判定、点名识别、water-fill 分批、lap floor、HELD、卡片领取条件与退避都写成纯函数。目录按领域划分，领域内部怎样分文件在开发时决定。

### Agent 运行

- `EngineAdapter`（`probe`、`classify`、`createAgentRuntime`）与 `AgentEngineRuntime`（`runTurn`、`shutdown`）保留现有的两层结构。错误用可辨识联合类型表示。
- 取消用 `AbortSignal`。Engine 以独立进程组启动（`spawn` 的 `detached: true`），取消时先发 SIGINT，2 秒后发 SIGKILL。
- Engine 的 JSONL 输出经过 `normalizeEvent` 转成统一的活动事件。做法来自 raft 的 `packages/daemon/src/drivers/` 中各 runtime 的 `*EventNormalizer.ts`。
- 每个 AgentRunner 是一个异步 actor，内部只有一个串行处理循环。全局并发上限用计数信号量实现。
- Seatbelt profile 由 TypeScript 生成，经 `/usr/bin/sandbox-exec -p` 启动 Engine。没有无沙箱的运行路径。
- shim 用 Electron 自带的可执行文件运行：`runtime/<session>/bin/openwork` 是一个脚本，以 `ELECTRON_RUN_AS_NODE=1` 执行 Electron 可执行文件与打包后的 `shim.js`。两者都在 `$HOME` 之外，沙箱可以读取。
- 第一版只接 OpenCode。功能对齐后，第一个新 Engine 是 Claude Code。

### 规则与文档

- 现有协作规则是移植的起点，不是冻结的契约。移植时可以简化或修改，并在同一改动里更新文档。
- `docs/subsystems/collaboration.md` 在移植到对应子功能时按需拆分，同时把 Rust 路径换成 TypeScript 路径。

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
- 假 Engine 是一个正式的 `EngineAdapter` 实现。
- 真实 OpenCode 与 Seatbelt 测试只在 macOS 上手动运行。
- 冒烟测试运行打包后的产物，不运行源码。shim 的冒烟测试用 Electron 可执行文件与打包后的 `shim.js`，在 Seatbelt 中启动。做法来自 DSH `docs/testing.md` 的“Test the real entry path”。
- 模型可见的文本（每轮 Turn 的输入、triage 题面、shim 的输出）用 vitest 文件快照逐字锁定。改动这些文本时，快照的 diff 进入代码审查。
- 日常检查：`pnpm lint`、`pnpm typecheck`。提交或合并前：`pnpm check`，它替代 `scripts/check.sh`。
- 提交前按改动选择能覆盖它的最小测试集，只汇报实际运行的命令。做法来自 DSH 的 `.agents/skills/dsh-pre-push-checks/SKILL.md`，写成本仓库的 `.agents/skills/` 中的一个 skill。
- 本地开发：`docker compose up -d` 后运行 `pnpm dev`。

### 移植顺序

每一步结束时应用都能启动和演示：

1. 骨架：workspace、protocol、Server 与 RuntimeSession、Electron supervisor，界面能连上 Server。同一步建立 `packages/AGENTS.md`、Biome、lefthook 与三个检查脚本，并把根 `AGENTS.md` 的“运行与检查”换成 TypeScript 的命令。
2. 人与人的聊天：Room 与 Message 端到端。同一步写提交前检查的 skill。
3. Agent 回复私聊：Computer、OpenCode adapter、Seatbelt、shim，以及 shim 的打包产物冒烟测试与 Turn 输入的快照。写 Computer 时，按 DSH `docs/defensive-patterns.md` 检查子进程与清理代码；本仓库的 `docs/defensive-patterns.md` 只记录实际出现过的缺陷。
4. 群聊协调：triage、点名路由、HELD、连发与逐字重复，以及 triage 题面的快照。
5. 看板：Board、Column、Card、领取与卡片唤醒。
6. 其余功能：Agenda、Climate、静音、运行观测。
7. 清理：删除 `crates/`、Tauri 与旧 `desktop/`，卸载 rust-analyzer 相关工具，更新 README、architecture.md、testing.md 与本 Agent Note。

功能对齐后接入 Claude Code 时，写一份“新增 Engine adapter”的操作指南，放在 `docs/cookbook/`。做法来自 DSH 的 `docs/cookbook/adding-an-llm-adapter.md`。

移植期间 TypeScript 代码放在根目录的 `packages/` 与 `apps/`。现有前端在第 1 步复制到 `apps/desktop`，旧目录在第 7 步删除。

## 考虑过的方案

**用内嵌 SQLite 替代 PostgreSQL。** 单用户、单写者时 SQLite 足够，也不需要 Docker。没有采用，因为以后可能部署到多机或云端，PostgreSQL 是之后最难替换的部分。另外 SQLite 的唯一约束不能延迟检查，看板排序要重写。

**本机默认用 PGlite，配置 `DATABASE_URL` 时用 PostgreSQL。** PGlite 是编译成 WASM 的 PostgreSQL，raft 用它跑测试（`packages/server/src/db/index.ts`）。它免去 Docker，SQL 方言不变。没有采用，选择统一使用真实 PostgreSQL。

**用 Server 进程内状态替代 Redis。** Node 单线程事件循环下，无需加锁就能原子地完成 claim。没有采用，因为以后 Server 多实例部署时，wake 与事件需要跨实例传递。

**Server 与 Computer 合并为一个进程，或全部放进 Electron 主进程。** 进程更少。没有采用，因为以后 Computer 要留在用户机器上，而 Server 可能迁到云端。全部放进主进程时，Engine 管理、数据库写入与界面通信无法隔离。

**Computer 守护进程用 Go。** 单个二进制便于分发到其他机器。没有采用：资源大头是 Engine 进程，守护进程的语言对内存与延迟影响很小。分发真正成为问题时，再按 HTTP 协议用 Go 重写它。

**守护进程用 Rust，Server 用 Go，只有前端用 TypeScript。** 一个功能要改两到三种语言，协议类型要在三种语言间同步，还要维护三套工具链。性能收益落不到用户可感知的地方，也与转向 TypeScript 的初衷相反。

**HTTP 框架用 Express。** raft 与 cumora 都用 Express（各自的 `package.json`）。没有采用，因为 Hono 能让 Server 的路由类型直接约束前端调用。

**保留 Tauri。** Tauri 使用系统 WebView，内存占用低于 Electron。没有采用，因为两个参考项目都用 Electron，而且 Electron 自带的 Node 运行时可以直接运行 shim。

**新建仓库重写。** 没有采用。在当前分支重写可以保留 git 历史与 `.agents/notes/` 中的决策记录。

**照搬 DSH 的完整文档与检查体系。** DSH 有 71 个 `scripts/verify-*` 检查、中英双语文档（`.md`、`.zh.md`、`.i18n.yaml`）、冻结的 Agent Note 归档、每个文件 100% 的覆盖率门槛和录制会话回放的快照。没有采用，因为这些服务于多人协作的大型仓库，维护成本超过单人项目的收益。只取三个检查脚本、git hooks 与代码约定。

## 验收条件

- 第 1 至 6 步每一步结束时，`docker compose up -d` 与 `pnpm dev` 能启动应用，已移植的功能可以演示。
- 第 3 步结束时，用户在私聊中给 Agent 发消息，Agent 在 Seatbelt 中运行 OpenCode，并经 shim 回复。
- Seatbelt 不可用时，Computer 不启动任何 Runner。
- 修改一个 Server 路由的响应字段后，`pnpm typecheck` 在使用该字段的前端代码处报错。
- `pnpm check` 通过：lint、类型检查、单元、集成与冒烟测试。
- 三个检查脚本各有一个测试，证明它会拒绝违规输入：失效的链接、状态行与目录不一致的 Agent Note、超出字数上限的根 `AGENTS.md`。
- `CLAUDE.md` 是指向 `AGENTS.md` 的软链接。
- 打包后的 shim 在 Seatbelt 中能调用 Server，由冒烟测试覆盖。
- 第 7 步结束后，仓库中没有 Rust 与 Tauri 代码，文档中没有 Rust 路径。

## 风险

- shim 依赖 Electron 的 `runAsNode` fuse。关闭这个 fuse 的打包配置会让 shim 无法启动。shim 在 Seatbelt 中经 `ELECTRON_RUN_AS_NODE` 启动的行为与启动耗时都没有实测。
- 未确认 drizzle schema 能否表达 `DEFERRABLE` 唯一约束。不能时，这两条约束写进手写的 SQL 迁移。
- Electron 的内存占用高于 Tauri。
- 演示前需要安装 Docker 并启动 PostgreSQL 与 Redis。
- 移植期间 Rust 与 TypeScript 两套代码并存。Rust 代码不再加功能，只修影响旧版本运行的问题。
- Hono 的生态小于 Express，参考项目中的 Express 路由写法需要转换。
