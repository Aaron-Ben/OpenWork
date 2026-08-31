<div align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/openwork-wordmark-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="docs/assets/openwork-wordmark-light.svg">
    <img src="docs/assets/openwork-wordmark-light.svg" alt="OpenWork" width="420">
  </picture>

  <p><strong>一个 Desktop，两种本机 Agent 工作方式</strong></p>
  <p>在工作台中完成可审阅的代码任务，或让多个本机 OpenCode Agent<br>通过房间、看板与 Agenda 持续协作。</p>

  <p>
    <img alt="Target" src="https://img.shields.io/badge/target-0.1.0-2563eb">
    <img alt="Status" src="https://img.shields.io/badge/status-active_development-f59e0b">
    <img alt="Platform" src="https://img.shields.io/badge/platform-macOS-111827">
    <img alt="License" src="https://img.shields.io/badge/license-Apache--2.0-0f766e">
    <img alt="Rust" src="https://img.shields.io/badge/backend-Rust-dea584">
    <img alt="Tauri" src="https://img.shields.io/badge/desktop-Tauri_2-24c8db">
  </p>

  <p>
    <a href="README.en.md">English</a> ·
    <a href="docs/README.md">文档</a> ·
    <a href="docs/architecture.md">架构</a> ·
    <a href="https://github.com/Aaron-Ben/OpenWork/issues">Issues</a>
  </p>
</div>

> [!IMPORTANT]
> OpenWork 尚未发布，当前面向 `0.1.0` 开发，只支持从源码运行。协作模式仅支持 macOS 与本机 OpenCode。OpenWork 的权限规则、Agent home 和 Runtime JWT 都不是 OS 安全沙箱；模型与工具进程仍拥有当前 macOS 用户授予它们的宿主机能力。

## OpenWork 是什么

OpenWork 是一个本地优先的桌面 Agent 工作空间。当前 Desktop 内有两条彼此独立的运行路径：

| 模式 | 适合什么 | 谁执行模型任务 | 核心对象 |
|---|---|---|---|
| **工作台** | 用户指定项目目录，逐个发起可审阅的代码与文件任务 | OpenWork 自己的 Agent Loop 和模型 Provider adapter | Session、Turn、Tool Call、Permission、Trace |
| **协作模式** | 创建多个长期存在的本机同事，让它们围绕消息和任务主动协作 | 每个协作 Agent 独立的本机 OpenCode Runner | Agent、Room、Message、Board、Card、Run |

两种模式共享同一个 Tauri Desktop、主题、i18n 和 PostgreSQL 实例，但不共享运行状态机。工作台中的只读 Sub-Agent 不是协作模式中的长期 Agent；工作台 Session 也不是协作 Room。

## 当前能力

### 工作台

- **显式模型选择**：内置 OpenAI、Anthropic、DeepSeek、Kimi、Qwen 和 GLM Provider 预设；每个 Session 选择具体 Provider 与模型，不做跨模型静默 fallback；
- **Agent Loop**：在一个 Turn 中推进 Model → Tool/Permission → Model，直到完成、失败、取消或触发保护条件；
- **七个内置工具**：`read`、`write`、`edit`、`grep`、`glob`、`list`、`bash`；文件工具统一经过路径授权边界，`bash` 在工作目录中启动宿主 POSIX Shell；
- **两种权限模式**：`default` 自动允许工作区读取和可证明只读的命令；`acceptEdits` 额外允许非敏感的工作区文件修改；
- **可审阅文件改动**：`write` / `edit` 产生结构化 Diff，并支持冲突检查下的 Undo / Reapply；
- **上下文工程**：支持 `AGENTS.md`、Skill、上下文构成预览、自动或手动 `/compact`、checkpoint、replay 与 rewind；
- **任务与只读 Sub-Agent**：复杂 Turn 可以维护任务清单，并派生一层只读 Sub-Agent 进行代码库调查；
- **质量 Trace**：记录模型实际请求、System Context、工具定义、Token、权限决定、耗时和失败阶段。

### 协作模式

- **长期 Agent roster**：创建、编辑、归档和恢复 Agent；每个 Agent 持久化 persona、主模型、triage 模型、Agenda 开关与 `engine_id`；
- **每 Agent 独立 Engine**：领域模型允许每个 Agent 选择自己的 Engine，当前唯一生产 adapter 是本机 `OpenCode`；模型 ID 直接交给 OpenCode，因此可以使用 OpenCode 支持的自定义模型；
- **私聊与群聊**：固定人类 Participant 为 `local-user`；Direct Room 幂等创建，Group Room 的成员只能由 Desktop 用户管理；
- **消息协调**：Agent 通过 durable inbox、triage、HELD 和 delivery settlement 协调响应；消息正文以 PostgreSQL 为事实来源；
- **Board / Column / Card**：用户管理看板结构和任务分配，Agent 通过结构化命令读取、创建、认领、更新和移动 Card；
- **Agenda**：开启后，Agent 可以依据未完成 Card 和停滞 Room 主动发起有界工作；
- **运行观测**：按 Agent 和状态检查每个 Turn 的 Run、模型、Token、耗时、错误与结构化事件时间线；
- **事件驱动 Desktop**：Room、Message、Board、Agent 和 Runner 变化通过失效事件刷新，业务真相始终由 Server 重新投影。

## 协作模式的运行边界

当前协作模式刻意保持为一个本机产品：

```text
一个 macOS 登录用户
└── 一个 OpenWork Desktop 生命周期
    ├── 一个 Collaboration Server
    ├── 一个由 Desktop 监督的 Computer daemon
    └── 多个本机 AgentRunner
        └── 每个 Agent 一个 OpenCode 子进程
```

- Server 和 Computer 是独立进程，但不会由 `launchd` 常驻；Desktop 启动它们，也负责停止它们；
- 每次 Desktop 启动都会创建新的 RuntimeSession、临时 Desktop/Computer 凭证和短期 Agent JWT；
- Server 或 Computer 意外退出时，Desktop 会成组替换两者并旋转 RuntimeSession；
- 正常退出 Desktop 时，先停止 Computer 和全部 Engine 进程，再停止 Server；PostgreSQL 与 Redis 外部服务不会被停止；
- `~/.openwork/runtime/<runtime-session-id>/` 保存临时 shim、token 和派生配置，正常退出时清除；
- `~/.openwork/agents/<agent-id>/` 保存持久 persona、私有 `work/` 与最小 Engine 会话连续性。

Agent 的 `work/` 彼此独立，不是多个 Agent 共享的真实项目 checkout。当前不提供远程 Mac、多 Computer 分配、后台常驻 Runtime、共享项目目录、Worktree、Memory、协作 Skill、reaction 或可视化 Agent 关系。

## 架构

```mermaid
flowchart TB
    UI["React Desktop"] -->|"Tauri Command / Event"| Host["Tauri Host"]

    Host --> Core["OpenWorkCore<br/>工作台 Runtime"]
    Core --> Models["Model adapters"]
    Core --> Tools["Tool + Permission Runtime"]
    Core --> PG[(PostgreSQL)]

    Host -->|"supervises"| Server["Collaboration Server"]
    Host -->|"supervises"| Computer["Local Computer daemon"]
    Computer -->|"HTTP + management SSE"| Server
    Computer --> Runners["per-Agent Runner"]
    Runners --> OpenCode["local OpenCode"]
    Server --> PG
    Server --> Redis[(Redis<br/>expiring coordination)]
```

协作分支的依赖关系是有意设计的：

- **Server** 是协作业务事实的唯一写者，拥有 PostgreSQL、Redis、Room、Board、Run、triage 和 Agenda；它不启动 Engine；
- **Computer** 对账 desired/actual Agent 状态，管理 Agent home、Engine adapter 和子进程；它不持有数据库凭证；
- **Desktop** 只监督生命周期并调用 typed command；它不复制 Server 的业务规则；
- **OpenCode** 只通过每个 Agent Runtime 注入的结构化 `openwork` shim 与 Server 交互。

## 数据放在哪里

| 位置 | 保存内容 | 生命周期 |
|---|---|---|
| PostgreSQL | Provider、Session、Message、Trace，以及协作 Agent、Room、Board、Run 和命令幂等结果 | 持久 |
| Redis | wake、seen、HELD、rate limit 与 cooldown | 可过期、可重建 |
| `~/.openwork/agents/` | 协作 Agent persona、私有工作文件、最小 Engine continuity | 持久 |
| `~/.openwork/runtime/` | 当前 RuntimeSession 的 shim、临时凭证和派生 Engine 配置 | 临时 |
| OpenCode data root | OpenCode 自己的登录态和 Provider 配置 | 由 OpenCode 管理 |

Redis 不保存消息正文、Board 或待执行任务。Redis 丢失最多导致额外的 poll/triage，不能删除 PostgreSQL 中的 durable facts。

## 快速开始

### 1. 环境要求

- macOS；
- Rust stable；
- Node.js 与 pnpm；
- Docker 与 Docker Compose，或可直接访问的 PostgreSQL 16 和 Redis；
- 当前平台的 [Tauri 2 系统依赖](https://v2.tauri.app/start/prerequisites/)；
- 已安装、完成 Provider 登录并可从当前终端执行的 `opencode` CLI。

### 2. 获取源码并配置环境

```bash
git clone https://github.com/Aaron-Ben/OpenWork.git
cd OpenWork
cp .env.example .env
openssl rand -base64 32
```

把生成的 Base64 值写入根目录 `.env`，并补充 Redis 地址：

```dotenv
DATABASE_URL=postgres://openwork:openwork@127.0.0.1:5432/openwork
REDIS_URL=redis://127.0.0.1:6379/0
OPENWORK_API_KEY_ENCRYPTION_KEY=<生成的 Base64 值>
```

> [!WARNING]
> 只要继续使用同一个数据库，就不要更换 `OPENWORK_API_KEY_ENCRYPTION_KEY`。更换后，数据库中已有的工作台 Provider API Key 将无法解密。

### 3. 启动 PostgreSQL 和 Redis

仓库内的 Compose 配置提供 PostgreSQL：

```bash
docker compose up -d postgres
```

如果本机还没有 Redis，可以启动一个只保存短期协调数据的容器：

```bash
docker run --rm -d --name openwork-redis -p 6379:6379 redis:7-alpine
```

如果已经运行 Redis，只需让 `REDIS_URL` 指向它。

### 4. 验证 OpenCode 并启动 Desktop

```bash
opencode --version
cd desktop
pnpm install
pnpm tauri dev
```

Desktop 命令必须在 `desktop/` 中执行；仓库根目录没有 `package.json`。Debug Desktop 会向上读取仓库根目录 `.env`，并在启动时应用工作台与协作数据库迁移；Release 构建不会自动读取开发环境的 `.env`，必须从启动环境显式注入这些变量。

## 第一次使用

### 工作台

1. 在 Settings 中创建 Provider 并保存 API Key；
2. 创建 Session，选择模型与工作目录；
3. 提交任务，并在需要时处理 Permission Request；
4. 从消息、文件 Diff、上下文窗口和 Trace 检查执行过程。

### 协作模式

1. 从工作台侧栏进入 Collaboration；
2. 创建 Agent，填写 persona、OpenCode 主模型与 triage 模型；
3. 打开 Agent 私聊，或创建 Group Room 并选择成员；
4. 创建 Board / Column / Card，需要主动工作时为 Agent 开启 Agenda；
5. 在“运行观测”中检查每个 Agent Turn 的状态和事件轨迹。

协作 Agent 使用 OpenCode 当前登录态；Collaboration Server 不保存 OpenCode 的 Provider API Key。

## 安全边界

| 边界 | 当前行为 |
|---|---|
| 工作台文件工具 | 解析真实路径并执行工作区授权；显式的工作区外访问需要当前 Execution Permit |
| 工作台 `bash` | 语法分析只用于权限判断，不提供执行期隔离；批准命令等于信任它及其子进程 |
| 协作 Agent home / JWT | 提供应用层身份、API 权限和状态隔离，不阻止同一 macOS 用户下的可信进程访问其他宿主文件 |
| OpenCode | 本机子进程，继承当前用户允许的文件与网络能力；没有 OpenWork OS 沙箱 |
| 网络 | OpenWork 不强制网络隔离；模型请求会发送给用户配置的 Provider 或 OpenCode Provider |
| Provider 凭证 | 工作台 API Key 使用 AES-256-GCM 加密后存入 PostgreSQL；OpenCode 凭证由 OpenCode 自己管理 |
| Trace | 可能包含私有代码、模型请求、命令和错误信息，应按敏感开发数据管理 |

只应在可信项目、可信本机 Agent 配置和理解其权限范围的前提下使用 OpenWork。

## 文档

- [文档索引](docs/README.md)
- [总体架构](docs/architecture.md)
- [工作台 Session Runtime](docs/session-runtime.md)
- [工具与权限](docs/tools.md) · [权限模型](docs/permissions.md)
- [上下文窗口](docs/context-window.md) · [压缩](docs/compaction.md) · [Trace](docs/trace.md)
- [Skill](docs/skills.md) · [只读 Sub-Agent](docs/multi-agent.md)
- [协作 Runtime](docs/collaboration.md)
- [协作 Desktop](docs/collaboration-desktop.md)
- [协作数据模型](docs/collaboration-data-model.md)

## 许可证

OpenWork 以 [Apache License 2.0](LICENSE) 开源。

---

发现问题或希望讨论设计时，请提交 [GitHub Issue](https://github.com/Aaron-Ben/OpenWork/issues)。
