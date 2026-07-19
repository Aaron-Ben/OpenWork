<div align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/openwork-wordmark-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="docs/assets/openwork-wordmark-light.svg">
    <img src="docs/assets/openwork-wordmark-light.svg" alt="OpenWork" width="420">
  </picture>

  <p><strong>本地优先、可追踪的桌面 Agent 工作台</strong></p>
  <p>用 Rust 驱动 Model → Tool/Permission → Model 循环，通过 Tauri Desktop 管理模型、会话、工具权限和运行 Trace。</p>

  <p>
    <img alt="Version" src="https://img.shields.io/badge/version-0.1.0-2563eb">
    <img alt="Status" src="https://img.shields.io/badge/status-active_development-f59e0b">
    <img alt="Rust" src="https://img.shields.io/badge/backend-Rust-dea584">
    <img alt="Tauri" src="https://img.shields.io/badge/desktop-Tauri_2-24c8db">
    <img alt="PostgreSQL" src="https://img.shields.io/badge/storage-PostgreSQL_16-4169e1">
  </p>

  <p>
    <a href="README.en.md">English</a> ·
    <a href="docs/README.md">文档</a> ·
    <a href="docs/redesign/README.md">架构</a> ·
    <a href="https://github.com/Aaron-Ben/OpenWork/issues">Issues</a>
  </p>
</div>

> [!IMPORTANT]
> OpenWork 目前处于 `0.1.0` 主动开发阶段，需要从源码运行。它提供工作目录和工具权限边界，但**不提供操作系统级沙箱**；请只在可信目录和可接受的权限配置中使用。

## OpenWork 是什么

OpenWork 是一个本地桌面 Agent 工作台。你可以选择模型和工作目录，让 Agent 在一个持续的 Session 中读取、搜索和修改文件、运行命令，并在需要时等待你的权限决定。

它关注的是一条清晰、可诊断的本地运行链：

- **多模型接入**：内置 OpenAI、Anthropic、DeepSeek、Kimi、Qwen 和 GLM Provider 配置；
- **持续 Agent Loop**：一次 Turn 可以经历多次模型调用和工具调用，直到完成、失败、取消或触发保护条件；
- **受控工具执行**：内置 `read`、`write`、`edit`、`grep`、`glob`、`list` 和 `bash`，受工作目录和 Permission Profile 约束；
- **可审阅的文件变更**：文件工具生成结构化 Diff，并支持冲突检查下的 Undo/Reapply；
- **本地持久化**：Provider、Model、Session、Turn、Message 和 Trace 统一保存在 PostgreSQL；
- **运行诊断**：Trace 展示 Model/Tool 调用、真实重试、Token、分段耗时、权限等待和采集完整度；
- **桌面体验**：Tauri 2 + React，支持简体中文、繁体中文和英文界面。

## 快速开始

### 1. 环境要求

- Rust stable
- Node.js 与 Corepack/pnpm
- Docker 与 Docker Compose
- 当前平台所需的 [Tauri 2 系统依赖](https://v2.tauri.app/start/prerequisites/)

如果本机还没有 pnpm：

```bash
corepack enable
corepack prepare pnpm@latest --activate
```

### 2. 获取源码并配置环境

```bash
git clone https://github.com/Aaron-Ben/OpenWork.git
cd OpenWork
cp .env.example .env
openssl rand -base64 32
```

把最后一条命令生成的值写入根目录 `.env`：

```dotenv
DATABASE_URL=postgres://openwork:openwork@localhost:5432/openwork
OPENWORK_API_KEY_ENCRYPTION_KEY=<生成的 Base64 值>
```

> [!WARNING]
> 只要继续使用同一个数据库，就不要更换 `OPENWORK_API_KEY_ENCRYPTION_KEY`。更换后，数据库中已有的 Provider API Key 将无法解密。

### 3. 启动 PostgreSQL 并迁移

在仓库根目录运行：

```bash
docker compose up -d postgres
cargo run -p openwork-core --bin openwork-migrate
```

### 4. 启动 Desktop

```bash
cd apps/desktop
pnpm install
pnpm tauri dev
```

桌面端命令必须在 `apps/desktop` 中执行；仓库根目录没有 `package.json`。`OpenWorkCore::bootstrap` 也会应用待执行 migration，独立迁移命令主要用于首次配置和数据库诊断。

## 基本使用流程

1. 在 Settings 中配置 Provider、Model 和 API Key；
2. 创建 Session，并显式选择 `providerId + model` 与工作目录；
3. 向 Agent 提交任务，在权限请求出现时选择允许或拒绝；
4. 在会话中审阅工具活动、文件 Diff，并按需 Undo/Reapply；
5. 从运行记录或会话入口打开 Trace，定位模型重试、工具耗时和失败阶段。

OpenWork 不会自动选择模型，也不会在 Provider 失败时静默切换到其他模型。

## 运行架构

```mermaid
flowchart LR
    UI["Tauri Desktop<br/>React + TypeScript"] -->|"Command / Event"| Core["OpenWorkCore"]
    Core --> Registry["Session Registry"]
    Registry --> Actor["SessionActor"]
    Actor --> Chat["Chat State Actor"]
    Actor --> Model["Model Adapters<br/>HTTP + SSE"]
    Actor --> Tools["Tool Runtime<br/>Permission + Workspace"]
    Core --> DB[("PostgreSQL")]
    Actor -. "best-effort signals" .-> Trace["Trace Recorder"]
    Trace --> DB
```

架构中的关键约束：

- `openwork-core` 是唯一运行时入口；
- 一个活动 Session 对应一个 `SessionActor`，同一时间最多推进一个 Turn；
- `openwork-chat-state` 是 Conversation 的唯一写入者；
- Desktop 只适配 Command/Event，不复制后端状态机；
- Trace 是 best-effort 诊断数据，失败不能推进或改变 Turn；
- 未完成 Turn 在进程重启后标记为 `interrupted`，不会自动重放工具。

## 仓库结构

| 路径 | 职责 |
| --- | --- |
| `apps/desktop` | Tauri 2 / React / TypeScript 桌面客户端 |
| `crates/openwork-core` | Core Facade、Session Runtime、PostgreSQL Storage 与 Trace |
| `crates/openwork-agent` | Agent Definition、System Prompt 与静态策略 |
| `crates/openwork-chat-state` | Conversation 单写者 Actor 与模型请求快照 |
| `crates/openwork-models` | 模型协议、Provider Adapter、HTTP/SSE Transport 与错误分类 |
| `crates/openwork-tools` | Tool Catalog、权限策略、文件/进程执行与结构化文件变更结果 |
| `docs/redesign` | 当前权威架构、实施状态和明确暂缓项 |

依赖保持单向：`openwork-models` 位于底层，`openwork-tools` 与 `openwork-chat-state` 依赖模型契约，`openwork-agent` 依赖工具契约，`openwork-core` 组合所有运行时能力，Tauri 位于最外层 Host 边界。

## 数据与安全边界

- Provider API Key 使用 AES-256-GCM、随机 Nonce 和版本化 Envelope 加密后存入 PostgreSQL；
- 主密钥只通过 `OPENWORK_API_KEY_ENCRYPTION_KEY` 注入，不写入数据库；
- Debug Desktop 会自动加载根目录 `.env`，Release 构建不会加载开发 `.env`；
- Permission `Allow` 不能绕过 `ToolSessionContext` 的路径和进程边界；
- 当前没有 OS 级 Sandbox，应用进程仍能访问其操作系统账户拥有的资源；
- Trace V0.1 默认只记录白名单化的状态、计数、大小、耗时和错误信息，不保存完整 Prompt、Provider Body 或 Tool Input/Output 原文。

底层 Adapter 也支持环境变量配置。常用变量名：

```bash
OPENWORK_API_KEY_ENCRYPTION_KEY=...
OPENAI_API_KEY=...
ANTHROPIC_API_KEY=...
KIMI_API_KEY=...
DEEPSEEK_API_KEY=...
DASHSCOPE_API_KEY=...
```

不要提交真实密钥。

## 开发与验证

Rust 命令在仓库根目录运行：

```bash
cargo test
cargo clippy --all-targets --all-features
cargo fmt
```

Desktop 命令在 `apps/desktop` 中运行：

```bash
pnpm test
pnpm build
pnpm tauri build
```

PostgreSQL 集成测试需要显式提供测试数据库，否则相关测试会提前返回：

```bash
TEST_DATABASE_URL=postgres://openwork:openwork@localhost:5432/openwork \
  cargo test -p openwork-core
```

## 当前边界

仍未完成：

- 自动生成 Rust → TypeScript Host Contract 和 Drift Check；
- Trace 的生产关闭入口及 Queue/数据库/Flush 完整降级验收；
- OS 级 Sandbox 和可靠的工具副作用对账；
- 自动化 Live Provider Smoke Test。

当前 `0.1.x` 不包含跨进程恢复未完成 Turn、Event Journal、Checkpoint、Memory、MCP、Plan、Skill、Compaction、Git 集成、仓库级 Diff 或 Worktree。完整边界以权威设计文档为准。

## 文档

- [文档索引](docs/README.md)
- [Runtime 重构与当前状态](docs/redesign/README.md)
- [项目结构](docs/redesign/01-project-structure.md)
- [Session Runtime 与事件模型](docs/redesign/02-event-update-model.md)
- [PostgreSQL Schema](docs/redesign/03-database-schema.md)
- [Trace 设计 V0.1](docs/redesign/04-trace-design.md)
- [Desktop 前端架构](docs/redesign/06-frontend-architecture.md)
- [本地 PostgreSQL 与 SQLx Migration](docs/local-postgres.md)

发现问题或希望讨论设计时，请提交 [GitHub Issue](https://github.com/Aaron-Ben/OpenWork/issues)。
