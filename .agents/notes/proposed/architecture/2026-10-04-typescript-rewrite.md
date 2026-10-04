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
| 前端 | React、Vite、Tailwind；组件用 shadcn/ui 的做法（Radix 原语加 cva 与 tailwind-merge，组件源码放在仓库里） |
| 界面数据 | TanStack Query |
| SSE 解析 | eventsource-parser |
| shim 参数解析 | commander |
| Markdown | react-markdown 与 remark-gfm；代码高亮用 rehype-highlight |
| Electron 构建与开发 | electron-vite |
| 安装包 | electron-builder，在单独的打包步骤建立 |

### 存储

- PostgreSQL 保存业务事实，Redis 保存可过期、可重建的协调数据。
- 两者都是必需依赖。Server 启动时连不上就报错退出，不降级。
- `compose.yaml` 同时定义 `postgres` 与 `redis`。`docker compose up -d` 起全部依赖。
- 表结构随功能逐步设计，用 drizzle schema 定义。迁移由 `drizzle-kit generate` 生成并提交，Server 启动时执行。表名不加前缀。
- Redis 读写集中在 Server 的一个模块里。
- 第 2 步的唤醒走 Server 进程内的事件。Server 改为多实例部署时，再改走 Redis pub/sub。
- 事务里不 await 网络调用或 Engine 调用。

### 写操作分层

初步建议每个写操作分三步：

1. 判断：纯函数。输入已加载的状态，输出允许、拒绝或要写入的内容。不访问数据库和 Redis。
2. 落库：一个事务。加锁，加载状态，调用判断，写入。
3. 通知：事务提交后尽力发布进程内事件与 SSE 失效提示。失败不回滚。

复杂的协调规则写成纯函数，脱离数据库测试。cumora 的 `server/src/agents/triage-core.ts` 是同样的做法。

### Agent 运行

初步建议：

- Engine adapter 参考现有 Rust 版的两层结构（adapter 与每个 Agent 的 runtime）、cumora 的 `server/src/agents/computer/engine.ts` 与 raft 的 `packages/daemon/src/drivers/`。
- 取消用 `AbortSignal`。Engine 以独立进程组启动，取消时先发 SIGINT，再发 SIGKILL。
- Seatbelt profile 由 TypeScript 生成，经 `/usr/bin/sandbox-exec -p` 启动 Engine。没有无沙箱的运行路径。
- shim 用 Electron 自带的可执行文件运行：以 `ELECTRON_RUN_AS_NODE=1` 执行 Electron 可执行文件与打包后的 `shim.js`。两者都在 `$HOME` 之外，沙箱可以读取。
- shim 的命令名是 `crew`，Agent 用它调用 Server（例如 `crew reply`）。Computer 的本机数据目录是 `~/.crew`。
- 第一版只接 OpenCode。

### 文档

- 新实现的子系统页随功能编写，放在 `docs/subsystems/`。怎样拆分页面，做到对应功能时决定。
- 现有的 `collaboration.md` 与 `collaboration-desktop.md` 描述 Rust 版，开发期间保留作参考，最后一步删除。
- `docs/architecture.md` 描述 Rust 版的 crate 与依赖方向，已经删除。第 2 步结束前为 Crew 新写一份，内容是进程、包、依赖方向与领域词汇，并在 `docs/AGENTS.md` 的分层表中恢复它。

### 仓库规则文件

做法来自 DSH（`/Volumes/Extreme SSD/Code/deepseek-harness`）：

- 根目录的规则文件是 `AGENTS.md`，`CLAUDE.md` 是指向它的软链接。Claude Code 与 Codex 读到同一份规则。DSH 的根 `CLAUDE.md` 就是指向 `AGENTS.md` 的软链接。
- 根 `AGENTS.md` 只放每个会话都需要的常驻规则。每条一到三行，并链接到理由。
- 新增 `packages/AGENTS.md`，只放 `packages/` 与 `apps/` 下的代码约定。`packages/CLAUDE.md` 与 `apps/CLAUDE.md` 是指向它的软链接，编辑这两个目录下的代码时 Claude Code 会加载它。`docs/AGENTS.md` 继续只放文档规则。做法来自 DSH 的 `packages/AGENTS.md`。

### 代码约定

以下约定写进 `packages/AGENTS.md`，来自 DSH 的根 `AGENTS.md` 与 `packages/AGENTS.md`：

- 跨进程或跨包传递的 ID（`AgentId`、`RoomId`、`RunId` 等）用 branded 类型，不用裸 `string`。
- 同一进程内信任 TypeScript 类型。只在 HTTP、文件、子进程输出、环境变量与配置处用 zod 校验。
- 可辨识联合用 `switch` 处理，封闭的联合以 `assertNever` 结尾。
- 只在操作成功后发通知、更新派生状态。
- 配置缺失或错误时，在启动时报错，不跳过。
- 禁止 `as unknown`。空的 `catch` 写明吞掉的错误和原因。

### 自动检查

能机械检查的规则，写成会被执行的检查脚本。DSH 用 `scripts/verify-*.ts` 检查文档规则，cumora 用 `scripts/guard-*.mjs` 检查跨文件一致性。第一批只有三个，放在 `scripts/`，由 `scripts/verify-docs.ts` 一次运行，`pnpm lint` 调用它：

- Markdown 相对链接指向存在的文件。
- Agent Note 的前三行格式正确，状态行与所在目录一致。
- 根 `AGENTS.md` 不超过字数上限。上限按非空白字符计，是 2,800 字（`scripts/agents-budget.ts`）。

git hooks 用 lefthook，只做快速检查，做法来自 DSH 的 `lefthook.yml`：

- pre-commit：对暂存文件运行 Biome，只检查、不修复；运行 `git diff --cached --check`。格式问题用 `pnpm format` 修复。
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

1. 骨架：范围见下文“第 1 步的实现决策”。
2. 最小私聊：用户给一个 Agent 发消息，Server 保存消息并唤醒 Agent，Computer 在 Seatbelt 中启动 OpenCode，Agent 经 shim 回复，界面显示回复。每个环节只做最简单的版本：一对一私聊，没有 triage、退避与幂等。范围见下文“第 2 步的实现决策”。同一步建立数据库迁移，并加入 shim 的构建产物冒烟测试、Turn 输入的快照与提交前检查的 skill。写 Computer 时，按 DSH `docs/defensive-patterns.md` 检查子进程与清理代码；本仓库的 `docs/defensive-patterns.md` 只记录实际出现过的缺陷。
3. 群聊协调：多个 Agent、triage、点名路由、HELD、连发与逐字重复，以及 triage 题面的快照。
4. 看板：Board、Column、Card、领取与卡片唤醒。
5. 其余功能：Agenda、Climate、静音、运行观测。
6. 打包：electron-builder 安装包，见下文“第 2 步的实现决策”中的打包部分。
7. 删除 Rust：删除 `crates/`、Tauri、旧 `desktop/`、`scripts/check.sh` 与描述 Rust 版的文档，卸载 rust-analyzer 相关工具，更新 README、testing.md 与本 Agent Note。

功能对齐后接入新的 Engine 时，写一份“新增 Engine adapter”的操作指南，放在 `docs/cookbook/`。做法来自 DSH 的 `docs/cookbook/adding-an-llm-adapter.md`。

新代码放在根目录的 `packages/` 与 `apps/`。界面在对应步骤设计，不复制现有的 `desktop/src`。

### 第 1 步的实现决策

**workspace 与 tsconfig**

- 包之间直接引用源码：每个包的 `package.json` 用 `"exports": { ".": "./src/index.ts" }`，包之间没有构建步骤。做法来自 raft 的 `packages/shared/package.json`（`main` 指向 `src/index.ts`）。
- 根目录 `tsconfig.base.json` 保存共用选项：`strict`、`noUncheckedIndexedAccess`、`noFallthroughCasesInSwitch`、`moduleResolution: bundler`、`noEmit`。每个包的 tsconfig `extends` 它。raft 每个包写一份完整的 tsconfig，没有共用部分。
- `apps/desktop` 有两份 tsconfig：`tsconfig.node.json` 覆盖主进程与 preload（Node 类型），`tsconfig.web.json` 覆盖渲染进程（DOM 类型与 JSX）。渲染进程的代码无法通过类型检查使用 Node API。
- 类型检查对每个包运行 `tsc --noEmit`（`pnpm -r typecheck`）。

**Electron 构建与开发**

- electron-vite 用一份配置构建主进程、preload 与渲染进程。`pnpm dev` 运行 `electron-vite dev --watch`：渲染进程热更新，主进程、preload、Server 或 Computer 的源码改动后重新构建并重启 Electron。不加 `--watch` 时主进程不会重新构建。
- preload 构建为 CommonJS（`index.cjs`），启用沙箱的窗口只能加载这种格式。
- 主进程在 `ELECTRON_RENDERER_URL` 存在时加载开发服务器，否则加载构建产物。cumora 用同一个环境变量（`electron/main.cjs`），由 `concurrently` 与 `wait-on` 拼出开发模式。

**主进程启动 Server 与 Computer**

- 主进程用 `child_process.spawn` 运行 Electron 可执行文件，设置 `ELECTRON_RUN_AS_NODE=1`，参数是 Server 或 Computer 的入口 JS。测试中用 `node` 运行同样的入口，监管代码不变。shim 使用同一种机制。
- 主进程向子进程的 stdin 写一行 JSON（bootstrap），子进程就绪后向 stdout 写一行 JSON（ready）。凭证不经过命令行参数与环境变量；Server 的随机端口由 ready 带回。这沿用 Rust 版的 `crates/openwork-collab/src/protocol/desktop.rs`。
- 启动 Computer 时，从环境变量中删除 `DATABASE_URL`、`REDIS_URL` 与 PostgreSQL 相关变量。Computer 只经 Server 的 HTTP 接口访问数据。Rust 版的做法见 `desktop/src-tauri/src/collab_client.rs` 的 `spawn_child`。
- Server 与 Computer 是 electron-vite 主进程配置的额外入口（`rollupOptions.input`），与主进程一起构建到 `out/main/`。主进程构建使用 `externalizeDeps: false`，把全部依赖打包进产物；否则 workspace 包会在运行时按 TS 源码加载而失败。
- `pg` 把 `pg-native` 声明为可选的 peer 依赖。没有安装时，Vite 在开发模式下用一个加载即抛错的模块代替它，所以 `electron.vite.config.ts` 中的插件把它解析成空模块。
- 主进程启动时读取根目录的 `.env`（`process.loadEnvFile`），并生成 RuntimeSession ID（`randomUUID`）与两个凭证（32 字节随机数的 base64url）。
- 子进程 30 秒内没有报告 ready 就按启动失败处理。停止时先停 Computer 再停 Server，每个先发 SIGTERM，3 秒后仍未退出就发 SIGKILL。
- 主进程保留每个子进程最后 8 KB 的 stderr，并加上 `[Server]` 或 `[Computer]` 前缀转发到自己的 stderr。
- 第 1 步只支持开发模式：没有 `ELECTRON_RENDERER_URL` 时主进程报错退出。
- 启动失败，或 Server、Computer 意外退出时，主进程停止整组进程，用系统错误对话框显示原因与 stderr 末尾，然后退出应用，不自动重启。
- Server 与 Computer 读完 bootstrap 后继续读取 stdin。stdin 结束说明主进程已经退出（包括被 SIGKILL），它们随之退出，不留下孤儿进程。

**界面连接 Server**

- 界面直接调用 Server，用 `hono/client` 获得有类型的客户端，请求带 `Authorization: Bearer <Desktop 凭证>`。
- SSE 用 `fetch` 读取，凭证放在请求头。浏览器的 `EventSource` 不能设置请求头。
- Server 的 CORS 只允许界面的来源。
- 主进程先启动 Server 与 Computer 并等到 ready，再创建窗口。preload 用 `ipcRenderer.sendSync` 向主进程索取一次地址与凭证，再经 `contextBridge` 只向页面暴露 `window.crew.serverUrl` 与 `window.crew.desktopToken`。凭证不经过 `additionalArguments`，因为那会把它放进渲染进程的命令行参数，本机其他用户可以用 `ps` 看到。窗口保持 Electron 的默认安全设置：上下文隔离、关闭 Node 集成、启用沙箱。
- 页面的 Content-Security-Policy 只允许本页面的资源、对 `127.0.0.1` 任意端口的请求，以及开发服务器的热更新连接。

**骨架的范围**

| 部分 | 第 1 步完成的内容 |
|---|---|
| Server | 在 `127.0.0.1` 的随机端口启动；启动时连接 PostgreSQL 与 Redis，失败就报错退出；一个需要 Desktop 凭证的健康接口 |
| Computer | 读取 bootstrap，用 Computer 凭证调用 Server 一次，证明凭证有效，再报告 ready |
| 主进程 | 按顺序启动两者，等待 ready，创建窗口；任一崩溃时停止整组，弹出错误对话框并退出 |
| 界面 | 一个页面，显示 Server 与 Computer 是否连上 |
| 工程 | workspace、tsconfig、electron-vite、Biome、lefthook、三个检查脚本、`packages/AGENTS.md`、根 `AGENTS.md` 中 TypeScript 的命令 |

- 数据库迁移在第 2 步出现第一张表时建立。
- 安装包在第 6 步（打包）建立。第 1 步只保证 `pnpm dev` 可以运行。
- 界面的布局、组件库与视觉风格在第 2 步设计聊天界面时讨论。第 1 步只搭好 React、Vite 与 Tailwind。

**其他**

- Node 24 与 pnpm 10，由 `.node-version` 与 `package.json` 的 `packageManager` 固定。raft 用同样的方式（根目录 `.node-version`，`packageManager: pnpm@10.29.3`）。
- 根包名是 `crew`，各包是 `@crew/protocol`、`@crew/server`、`@crew/computer` 与 `@crew/desktop`。
- 每个包用 `vitest run` 运行测试，根目录用 `pnpm -r test` 运行全部测试。
- 第 1 步的 `protocol` 包只包含 bootstrap 与 ready 消息的 schema，以及几个 branded ID 类型。

### 第 2 步的实现决策

**数据模型**

```text
users               id, display_name, created_at
agents              id, display_name, persona, engine_id, model, created_at
rooms               id, kind(direct), direct_key（唯一）, next_seq, created_at
room_users          room_id, user_id（组合主键）
room_agents         room_id, agent_id（组合主键）
messages            id, room_id, seq, author_user_id, author_agent_id, body, created_at
                    （room_id 与 seq 唯一；两个作者列恰好一个不为空）
agent_read_cursors  agent_id, room_id, last_read_seq（组合主键）
```

- 用户与 Agent 分成两张表。本机只有一个用户，由一行固定的记录表示。成员关系也分成 `room_users` 与 `room_agents`，做法来自 raft 的 `channel_humans` 与 `channel_agents`（`packages/server/src/db/schema.ts`）。
- 消息的作者用两个可空外键加“恰好一个不为空”的约束，数据库保证作者真实存在。
- 消息序号按房间递增：写入时锁住房间行，`next_seq` 加一。同一房间内序号连续、不跳号，Agent 的已读位置才可靠。同一房间的写入因此排队，不同房间互不影响。
- Agent 的已读位置单独存在 `agent_read_cursors`，做法来自 raft 的 `agent_channel_read_cursors`。
- ID 用 UUID，由 PostgreSQL 的 `gen_random_uuid()` 生成。代码里用 branded 类型区分 `UserId`、`AgentId`、`RoomId` 与 `MessageId`。
- 时间戳用 `timestamptz`。
- 第 3 步群聊需要的 `handle`（`@alice`）、triage、投递与运行记录，到对应步骤再加。
- 迁移文件由 `drizzle-kit generate` 生成到 `packages/server/drizzle/`。electron-vite 构建时把它复制到 `out/main/drizzle`，主进程经环境变量 `CREW_MIGRATIONS_DIR` 告诉 Server 迁移目录，测试直接传入路径。

**唤醒**

- Computer 整体建立一条 SSE 连接（`/computer/events`）。事件只说明哪个 Agent 可能有新消息，或 Agent 列表变了，不带消息正文。
- Computer 用 `GET /computer/agents/:agentId/inbox` 取已读位置之后的消息，Turn 成功后用 `POST /computer/agents/:agentId/inbox/ack` 推进已读位置。Turn 失败时已读位置不动，下次唤醒重新处理，消息可能被处理两次，但不会丢。
- 每次连接或重连成功后，Computer 为每个 Agent 读一次 inbox，补上断线期间丢失的事件。第 2 步不做定时轮询。
- Computer 连接时用 `GET /computer/agents` 取 Agent 列表，收到“列表变了”的事件时重新获取。
- Server 内部用进程内事件把“消息已写入”传给 SSE 连接。

**Engine**

- 每个 Turn 启动一次 `opencode run --pure --format json --auto --session <id> --model <模型>`，prompt 写入 stdin，工作目录是 Agent 的 `work/`。`--auto` 自动批准文件与命令操作，安全边界是 Seatbelt。raft（`packages/daemon/src/drivers/opencode.ts`）与 cumora（`server/src/agents/computer/engine.ts`）都按 Turn 启动 OpenCode。
- 一个 Turn 处理一次唤醒时的全部未读消息，Agent 可以在其中多次调用 `crew reply`。Agent 正在运行时到达的消息不立即处理，Turn 结束后合并成一轮再处理。第 2 步不做 cumora 的 2.5 秒等待。
- session ID 存在 `~/.crew/agents/<id>/engines/opencode/session.json`，下次以 `--session` 继续同一段对话。模型或规则变化时开新 session。
- Agent 目录是 `~/.crew/agents/<id>/`：`AGENTS.md`（身份与规则）、`work/`、`engines/opencode/data/`（OpenCode 的数据目录，每个 Agent 独立）。
- `AGENTS.md` 只写身份、人设、用 `crew reply` 发言（文字输出没人看到）与可以保持沉默。`OPENCODE_CONFIG_CONTENT` 的 `instructions` 指向它。
- 每轮 prompt 只有唤醒说明、当前时间与按房间分组的未读消息。
- 每次启动 OpenCode 前，Computer 在沙箱外读取用户的 `~/.local/share/opencode/auth.json`，经环境变量传入。raft 与 cumora 让 OpenCode 自己读这个文件，因为它们不把 OpenCode 关进沙箱。用户重新登录后，下一轮自动使用新凭证；创建 Agent 不涉及凭证。
- 第 2 步只从输出中取三样：成功与否、session ID、失败时的错误信息。
- 正式 Turn 没有总时长上限。停止时向整个进程组发 SIGINT，2 秒后发 SIGKILL。raft 只结束单个进程（`drivers/runtimeSession.ts`），OpenCode 启动的子命令可能残留。每个 Agent 同一时间只跑一个 Turn。
- 环境变量 `OPENCODE_AUTH_CONTENT` 与 `OPENCODE_CONFIG_CONTENT` 沿用 Rust 版的做法，实现时先在 OpenCode 1.18.18 上验证；输出中 session ID 的字段名同样待验证。

**沙箱与 shim**

- Seatbelt 规则与 Rust 版相同：写入只放行本 Agent 的目录、运行期目录、系统临时目录与几个设备；`$HOME` 之外全部可读，`$HOME` 之内只能读本 Agent 的目录与运行期目录；只拒绝读取内容，不拒绝 `stat`；网络不限制。Computer 启动时自检，沙箱不可用时不启动任何 Agent。
- shim 第 2 步只有 `crew reply <room-id>` 与 `crew --help`。正文只从 stdin 读取，Agent 用带引号的 heredoc（`<<'EOF'`）传入，shell 不改动其中的字符。参数解析用 commander。
- shim 的失败一律退出码 1，向 stderr 写一行英文的 `error: …`，说明原因与下一步。Server 的错误文本是中文且界面也在用，由 shim 按状态码翻译。请求超时 10 秒，不重试。
- 每个 Agent 有一个随机凭证：Computer 用 Computer 凭证向 Server 申请，写入只有该 Agent 能读的运行期文件。Server 在内存中记录凭证对应的 Agent，应用重启后失效。shim 带着它调用 `POST /agent/reply`。Server 确认 Agent 是房间成员后，锁房间行、分配序号、写入消息，提交后通知界面。
- shim 的代码在 `packages/computer/src/shim/`，作为 electron-vite 主进程配置的另一个入口构建到 `out/main/shim.js`。Computer 在运行期目录生成 `bin/crew` 包装脚本，用 `process.execPath`（Electron 可执行文件）加 `ELECTRON_RUN_AS_NODE=1` 运行它，并把这个 `bin` 放到 Agent 的 `PATH` 最前面。

**界面**

- 两栏布局：左侧是 Agent 列表（每个 Agent 对应一个私聊房间，带状态点），右侧是聊天。cumora 的左侧导航栏（`src/desktop/Rail.tsx`）在有多个页面时再加。
- 视觉风格：框架取“控制台”方向（侧栏、顶栏、状态与元信息用等宽字体，绿色强调色只用于回复中、选中与主按钮），正文取“纸面”方向（消息不加气泡，名字和时间在上，正文 15px、行距 1.8、最宽 680px）。浅色与深色两套主题跟随系统切换。颜色与字体是 `apps/desktop/src/renderer/index.css` 中的设计变量，只用系统字体。
- 系统的红黄绿按钮放进侧栏顶部（`titleBarStyle: "hiddenInset"`）。窗口只显示界面自己的页面：消息里的 http 与 https 链接交给系统浏览器打开，其余导航一律丢弃（`apps/desktop/src/main/navigation.ts`）。
- 组件按 shadcn/ui 的做法，第 2 步只需要按钮、输入框、文本域与对话框。
- 来自 Server 的数据用 TanStack Query 获取与缓存。收到 SSE 失效提示时，`invalidateQueries` 让对应的数据重新获取。cumora（`src/stores/messages.ts` 的 `applyEvent`）与 raft 把推送来的消息正文合并进 zustand store，每种事件都要写合并逻辑。
- Server 提供 `/desktop/events`（SSE），只发“某房间有新消息”与“Agent 列表或状态变了”两类提示。SSE 的解析用 eventsource-parser，重连循环自己写（从 1 秒开始指数退避，最长 30 秒），放在 protocol 包中，Computer 与界面共用。推送代码集中在一个模块里，以后改用 WebSocket 时只改这里。
- 新建 Agent 时填名字、人设与模型。Computer 启动时运行 `opencode models`，把可用模型列表上报给 Server，界面用下拉框显示。
- Server 在内存中保存每个 Agent 的状态：空闲、回复中、出错（附原因）。Computer 在 Turn 开始、结束与失败时上报。界面在聊天底部显示“正在回复”，出错时在对话里写明原因与下一步。失败的 Turn 不推进已读位置，下一条消息到来时自动重试，所以不需要单独的重试按钮。
- 消息正文按 Markdown 渲染，不渲染原始 HTML。代码块按围栏上写的语言高亮，不自动猜语言；高亮颜色同样是设计变量。highlight.js 输出类名，颜色交给 CSS，这一点与 cumora（`src/components/Message.tsx`）相同；raft 用 shiki（`packages/web/src/components/markdown/shikiHighlighter.ts`），它异步加载，配色来自自带主题。
- 侧栏底部显示界面与 Server 的 SSE 连接状态。没有“Computer 是否连接”的显示：主进程等 Computer 连上 Server 后才打开窗口，Computer 退出时整个应用随之退出，所以窗口存在期间它总是已连接。

**打包**

- 第 2 步不做安装包，用 `pnpm dev` 运行。shim 的冒烟测试用 `electron-vite build` 的产物。
- `apps/desktop/package.json` 现在就设置 `"productName": "Crew"`，应用数据目录因此是 `~/Library/Application Support/Crew`。
- 第 6 步打包时：用 electron-builder；Server、Computer、shim 与迁移文件经 `extraResources` 放在 asar 之外，做法来自 raft 把 CLI 放在 `Resources/cli/index.js`（`apps/raft-desktop-electron/electron-builder.yml`）；界面经自定义协议 `app://crew` 加载，CORS 来源随之改为它，raft 与 cumora 都用 `app://`；保留 `runAsNode` fuse；打包后的配置来源届时再定。

## 考虑过的方案

**用内嵌 SQLite 替代 PostgreSQL。** 单用户、单写者时 SQLite 足够，也不需要 Docker。没有采用，因为以后可能部署到多机或云端，PostgreSQL 是之后最难替换的部分。

**本机默认用 PGlite，配置 `DATABASE_URL` 时用 PostgreSQL。** PGlite 是编译成 WASM 的 PostgreSQL，raft 用它跑测试（`packages/server/src/db/index.ts`）。它免去 Docker，SQL 方言不变。没有采用，选择统一使用真实 PostgreSQL。

**用 Server 进程内状态替代 Redis。** Node 单线程事件循环下，无需加锁就能原子地完成 claim。没有采用，因为以后 Server 多实例部署时，协调数据与事件需要跨实例共享。第 2 步的唤醒先走进程内事件，是因为它只需要单个 Server 进程。

**Server 与 Computer 合并为一个进程，或全部放进 Electron 主进程。** 进程更少。没有采用，因为以后 Computer 要留在用户机器上，而 Server 可能迁到云端。全部放进主进程时，Engine 管理、数据库写入与界面通信无法隔离。

**Computer 守护进程用 Go。** 单个二进制便于分发到其他机器。没有采用：资源大头是 Engine 进程，守护进程的语言对内存与延迟影响很小。分发真正成为问题时，再按 HTTP 协议用 Go 重写它。

**守护进程用 Rust，Server 用 Go，只有前端用 TypeScript。** 一个功能要改两到三种语言，协议类型要在三种语言间同步，还要维护三套工具链。性能收益落不到用户可感知的地方，也与转向 TypeScript 的初衷相反。

**HTTP 框架用 Express。** raft 与 cumora 都用 Express（各自的 `package.json`）。没有采用，因为 Hono 能让 Server 的路由类型直接约束前端调用。

**保留 Tauri。** Tauri 使用系统 WebView，内存占用低于 Electron。没有采用，因为两个参考项目都用 Electron，而且 Electron 自带的 Node 运行时可以直接运行 shim。

**新建仓库。** 没有采用。在当前分支开发可以保留 git 历史与 `.agents/notes/` 中的决策记录。

**照搬 DSH 的完整文档与检查体系。** DSH 有 71 个 `scripts/verify-*` 检查、中英双语文档（`.md`、`.zh.md`、`.i18n.yaml`）、冻结的 Agent Note 归档、每个文件 100% 的覆盖率门槛和录制会话回放的快照。没有采用，因为这些服务于多人协作的大型仓库，维护成本超过单人项目的收益。只取三个检查脚本、git hooks 与代码约定。

**按进程逐个替换 Rust 代码。** 先用 Electron 与新界面替换 Tauri，拉起现有的 Rust Server 与 Computer（`openwork-collab server|computer`）；再依次用 TypeScript 替换 Computer 与 Server。每个阶段应用都完整可用。没有采用：TypeScript 一侧在替换完成前必须兼容 Rust 的线协议，不能从零设计接口；而目标是参考三个项目做一个自己的实现。

**DSH 式 project references。** DSH 的 `tsconfig.base.json` 启用 `composite`，每个包先构建声明文件到 `lib/types`，再由 `references` 声明依赖。没有采用：它服务于五十多个包、npm 发布与增量构建，四个不发布的包用不上，还要处理过期的构建产物。

**单个 package。** cumora 用一个 package 加 `@/*` 路径别名。没有采用：渲染进程可以直接导入 Server 的内部代码，也与已经确定的包划分矛盾。

**自己组合 tsup、Vite、concurrently 与 wait-on。** 每一步都可见，tsup 也能打包 Server 与 Computer。没有采用：主进程改动后关闭旧 Electron、启动新 Electron 的逻辑要自己写，这正是 electron-vite 已经处理的部分。

**用 `utilityProcess` 启动 Server 与 Computer。** 由 Electron 管理子进程。没有采用：入口依赖 Electron 的 API，测试也必须启动 Electron。

**重新启动自身并带上角色参数。** raft（`apps/raft-desktop-electron/src/app/index.ts` 的 `findHeadlessMode`）与 Tauri 版都这样做。没有采用：入口必须在任何界面代码之前识别角色参数，raft 的注释记录了漏掉参数时子进程打开第二个窗口的缺陷。

**界面经主进程转发调用 Server。** Tauri 版这样做，凭证不进入页面。没有采用：每个接口多一层 IPC，`hono/client` 的端到端类型断开。页面被攻击时，攻击者同样可以让主进程替它转发请求，转发带来的隔离有限。

**由主进程拦截请求并自动加上凭证。** Electron 的 `session.webRequest` 可以修改页面发出的请求头，凭证不进入页面，`hono/client` 仍然可用。没有采用：页面代码看不出凭证从哪里来，排查与讲解都更难。

**把凭证放在 SSE 的 URL 中。** 可以继续用浏览器的 `EventSource`。没有采用：凭证会出现在 URL 与日志中。

**先做人与人的聊天，再接 Agent。** 没有采用。人与人的聊天不是产品的核心，按这个顺序要到第三步才能演示 Agent 回复。最小私聊让第二步就能演示核心效果。

**用户与 Agent 共用一张参与者表。** Rust 版与 cumora 这样做，消息与成员只需要一个外键。没有采用：用户与 Agent 的属性差别大，分表后各自的约束更清楚；外键完整性改由“两个可空外键”保证。

**消息作者用 `sender_type` 加 `sender_id`。** raft 的 `messages` 这样做，写起来简单。没有采用：`sender_id` 没有外键，类型与 ID 不匹配、作者不存在或已删除时数据库都不会拒绝。

**全局自增的消息序号。** raft 用 `bigserial`，不需要锁房间行。没有采用：并发写入时序号可能与提交顺序不一致，读者按“某序号之后”取消息会漏掉后提交的小序号，raft 为此另有 `packages/sync-core` 检查缺号。

**Agent 的已读位置放在成员表上。** Rust 版的 `room_members.last_read_seq` 这样做，少一张表。没有采用：用户与 Agent 分表后成员表也分成两张，单独的已读表更自然，成员关系与每轮都在更新的已读位置也就分开了。

**由代码生成 UUID。** 插入前就知道 ID，便于乐观更新与重试去重。没有采用：第 2 步还没有这类需求，数据库生成更简单；需要时只要把默认值换成代码里的一行。

**每个 Agent 一条 SSE 连接。** Rust 版与 cumora（`server/src/agents/runtime/server.ts` 的 `/wake-stream`）这样做，因为它们的每个 Agent 是独立进程或容器，持有自己的凭证。没有采用：Crew 只有一个 Computer 进程管理全部 Agent。

**用 WebSocket 推送消息正文并要求确认。** raft 的 `agent:deliver` 与 `agent:deliver:ack`（`packages/shared/src/index.ts`）这样做。raft 的 Server 在云端，要向用户机器发送十几种命令并等待回答，需要双向通道。没有采用：Crew 的 Server 只需要向 Computer 与界面单向发出“有新东西”的提示，推送正文还要自己实现确认与重发。Server 需要向客户端提问并等待回答时，再改用 WebSocket。

**用 Socket.IO。** 自带重连、房间与确认。没有采用：它有自己的协议，前后端都必须使用它；单向的失效提示用 SSE 就够了。

**第 2 步就让唤醒走 Redis pub/sub。** 改为多实例时不用再改。没有采用：第 2 步只有一个 Server 进程，进程内事件更简单；唤醒代码集中在一个模块里，以后替换成本低。

**shim 经 Computer 的本地代理调用 Server。** cumora 用文件 IPC 交给 daemon，raft 用本地凭证代理（`packages/daemon/src/agentCredentialProxy.ts`），模型看不到凭证。第 2 步没有采用：Computer 要多一个本地服务与转发层。以后需要防止 Agent 直接使用自己的凭证时再考虑。

**shim 也接受写在命令行上的正文。** cumora 的 `reply <convo_id> "<body>"` 这样做（`server/src/agents/cli.ts`）。没有采用：命令行上的正文先经过 shell，反引号与 `$` 会被展开，消息被悄悄改写；用单引号时 `\n` 又不会变成换行，cumora 为此写了 `unescapeChat`，再用 `--stdin` 与 `--file` 绕开它对代码片段的破坏（`server/src/agents/cli-parse.ts`）。raft 只接受 stdin（`packages/cli/src/commands/message/send.ts`）。

**用模型最后的文字输出作为回复。** 第 2 步可以省掉 shim。没有采用：shim 已经验证可行，它让 Agent 可以选择沉默，也为第 3 步群聊的协调留出余地。

**界面数据用 zustand store 手动管理。** raft 与 cumora 这样做。没有采用：它们推送消息正文，需要逐个事件合并；Crew 只推送失效提示，TanStack Query 的“失效后重新获取”正好对应。

**模型名用文本框填写。** 简单，但容易填错。没有采用：Computer 运行 `opencode models` 就能拿到可用列表。

**第 2 步建立安装包。** 原计划用打包产物做 shim 冒烟测试。没有采用：冒烟测试用 `electron-vite build` 的产物就能覆盖 shim 在沙箱中的运行；“asar 内的 shim 能否读取”由打包时放在 asar 之外解决；安装包带来的自定义协议、打包后的配置与签名问题第 2 步用不上。

## 验收条件

- 第 1 至 6 步每一步结束时，`docker compose up -d` 与 `pnpm dev` 能启动应用，已完成的功能可以演示。
- 第 2 步结束时，用户在私聊中给 Agent 发消息，Agent 在 Seatbelt 中运行 OpenCode，并经 shim 回复。
- Seatbelt 不可用时，Computer 不启动任何 Agent。
- 修改一个 Server 路由的响应字段后，`pnpm typecheck` 在使用该字段的前端代码处报错。
- `pnpm check` 通过：lint、类型检查、单元、集成与冒烟测试。
- 三个检查脚本各有一个测试，证明它会拒绝违规输入：失效的链接、状态行与目录不一致的 Agent Note、超出字数上限的根 `AGENTS.md`。
- `CLAUDE.md` 是指向 `AGENTS.md` 的软链接。
- 构建后的 shim（`electron-vite build` 的产物）在 Seatbelt 中能调用 Server，由冒烟测试覆盖。
- 第 7 步结束后，仓库中没有 Rust 与 Tauri 代码，文档中没有 Rust 路径。

## 风险

- shim 依赖 Electron 的 `runAsNode` fuse。关闭这个 fuse 的打包配置会让 shim 无法启动。2026-10-04 的验证中，shim 在与 Rust 版同构的 Seatbelt 规则下经 `ELECTRON_RUN_AS_NODE` 启动，能调用本机 HTTP 服务，单次约 180 ms（系统 Node 约 114 ms）。沙箱只拒绝了 `~/.CFUserTextEncoding` 的读取与 `/dev/dtracehelper` 的写入，都不影响运行。
- Agent 能读到自己的 Agent 凭证，可以绕过 OpenCode 直接以自己的身份调用 Server。沙箱保证它读不到其他 Agent 的凭证。
- 界面收到失效提示后重新获取整个消息列表。数据多时要改为只取某个序号之后的消息。
- Server 迁到云端后，反向代理默认缓冲响应会推迟 SSE 事件，需要关闭缓冲。
- Electron 的内存占用高于 Tauri。
- 演示前需要安装 Docker 并启动 PostgreSQL 与 Redis。
- 开发期间 Rust 与 TypeScript 两套代码并存。Rust 代码不再加功能，只修影响旧版本运行的问题。
- Hono 的生态小于 Express，参考项目中的 Express 路由写法需要转换。
- Desktop 凭证存在页面的 JS 中。页面被注入脚本时，凭证可以被读取。凭证只在当前 RuntimeSession 内、只在 loopback 上有效。
- 应用启动时，窗口要等 Server 就绪后才出现。
- 从零实现时，现有 Rust 版已经解决的问题可能重新出现。做到对应功能时，先查现有的子系统页与 Agent Note。
