<div align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/openwork-wordmark-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="docs/assets/openwork-wordmark-light.svg">
    <img src="docs/assets/openwork-wordmark-light.svg" alt="OpenWork" width="420">
  </picture>

  <p><strong>可审阅的本地桌面 Agent 工作台</strong></p>
  <p>在本地管理 Session、工具执行与 Trace，调用你显式选择的模型 Provider，<br>在指定目录中完成代码和文件任务。</p>

  <p>
    <img alt="Target" src="https://img.shields.io/badge/target-0.1.0-2563eb">
    <img alt="Status" src="https://img.shields.io/badge/status-active_development-f59e0b">
    <img alt="License" src="https://img.shields.io/badge/license-Apache--2.0-0f766e">
    <img alt="Rust" src="https://img.shields.io/badge/backend-Rust-dea584">
    <img alt="Tauri" src="https://img.shields.io/badge/desktop-Tauri_2-24c8db">
    <img alt="PostgreSQL" src="https://img.shields.io/badge/storage-PostgreSQL_16-4169e1">
  </p>

  <p>
    <a href="README.en.md">English</a> ·
    <a href="docs/README.md">文档</a> ·
    <a href="docs/architecture.md">架构</a> ·
    <a href="https://github.com/Aaron-Ben/OpenWork/issues">Issues</a>
  </p>
</div>

> [!IMPORTANT]
> OpenWork 尚未发布，当前面向 `0.1.0` 开发，只支持从源码运行。`bash` 以当前操作系统用户的权限在宿主机执行；权限规则与审批不是 OS 沙箱。请仅用于可信项目，并了解自动放行规则和宿主机访问范围。

## OpenWork 是什么

OpenWork 是一个本地桌面 Agent 工作台。你选择模型和工作目录后，一个持续的 Agent Loop 会在同一 Turn 内完成 Model → Tool/Permission → Model 循环，直到任务完成、失败、取消或触发保护条件。

应用运行时、Session、Message 和 Trace 保存在本机；模型推理请求会发送给你配置的 Provider。OpenWork 不是离线模型运行器，也不会替你自动选择或切换模型。

## 当前能力

- **显式模型选择**：内置 OpenAI、Anthropic、DeepSeek、Kimi、Qwen 和 GLM Provider 预设；每个 Session 由用户选择具体模型，不做跨模型静默 fallback；
- **七个内置工具**：`read`、`write`、`edit`、`grep`、`glob`、`list`、`bash`。前六个文件工具通过统一入口解析真实路径并核对授权；`bash` 在工作目录中启动宿主 POSIX Shell；
- **两种权限模式**：`default` 自动放行工作区读取和可证明只读的命令；`acceptEdits` 额外放行非敏感的工作区文件改动。无法证明的命令仍要用户确认；
- **可审阅文件改动**：`write` 和 `edit` 产生结构化 Diff，支持冲突检查下的 Undo / Reapply；通过 `bash` 发生的文件变化目前不具备同等级别的可靠对账；
- **可检查上下文**：工作目录根部的 `AGENTS.md` 会进入 System Context，Desktop 可以查看下一次模型调用的上下文构成与预算；
- **上下文压缩**：接近窗口上限时自动压缩，也可在空闲 Session 中显式执行 `/compact`；摘要、checkpoint、只读 replay 和 durable rewind 均持久化；
- **质量 Trace**：查看模型实际收到的请求、System Context、工具定义、响应引用、Token、权限决定和失败阶段；
- **本地持久化**：Provider、凭证、Session、Turn、Message、Compaction 和 Trace 使用 PostgreSQL；API Key 经 AES-256-GCM 加密；
- **三语界面**：简体中文、繁体中文和英文。

## 为什么可审阅

| 你想确认的事情 | OpenWork 当前提供的证据 |
|---|---|
| 模型这次实际看到了什么 | Trace 保存组装后提交的请求、System Context 和工具定义，而不是事后推测 |
| 一次工具调用为什么被允许或询问 | Tool Call Trace 记录权限模式、决定来源、规则或只读证明 |
| `write` / `edit` 改了什么 | Tool Result 携带 before/after 哈希与结构化 Diff，可执行 Undo / Reapply |
| 长对话压缩后还剩什么 | checkpoint 保存摘要、事实边界和运行状态提醒，原始 Message 不会因压缩被删除 |
| 慢在哪里、为什么失败 | Model / Tool / Compaction Span 记录分段耗时、错误阶段和 Token |

Trace 是排查和质量判断的证据，不参与推进 Turn，也不用于推断未知的工具副作用或自动重放工具。

## 快速开始

需要：

- Rust stable；
- Node.js 与 pnpm；
- Docker 与 Docker Compose；
- 当前平台的 [Tauri 2 系统依赖](https://v2.tauri.app/start/prerequisites/)。

获取源码并准备环境变量：

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

启动 PostgreSQL、执行迁移并启动 Desktop：

```bash
docker compose up -d postgres
cargo run -p openwork-core --bin openwork-migrate
cd desktop
pnpm install
pnpm tauri dev
```

Desktop 命令必须在 `desktop/` 中执行；仓库根目录没有 `package.json`。Debug Desktop 会读取根目录 `.env`，Release 构建不会读取开发环境的 `.env`。

启动后，在 Settings 中配置 Provider 与 API Key，创建 Session 并选择模型和工作目录，然后提交任务。权限请求会在需要时暂停当前 Turn；文件工具的结构化改动和 Trace 可以从会话界面继续检查。

## 权限与安全边界

| 边界 | 当前行为 |
|---|---|
| 六个文件工具 | 统一解析并检查真实目标或父目录；工作区内路径不能借相对路径或符号链接静默越界，显式的工作区外访问需要本次 Execution Permit |
| `default` | 自动读取工作区文件并执行通过封闭白名单证明为只读的命令；普通写入和其他命令需要确认 |
| `acceptEdits` | 额外自动允许非敏感工作区 `write` / `edit`，以及受限的文件系统命令形式和输出重定向；其他命令仍需确认 |
| `bash` | 语法分析只用于权限判断，不是执行期隔离；批准 `bash` 等于信任该命令及其启动的程序 |
| 网络 | 不强制限制；宿主进程可以使用操作系统账户拥有的网络能力 |
| 凭证 | API Key 加密后存入 PostgreSQL；主密钥只从环境变量注入，不写入数据库 |
| Trace 正文 | 始终记录全部支持的正文槽位，可能保存私有代码与模型请求；默认保留期为 30 天 |

OpenWork **有意不提供 OS 级沙箱、网络管控、无人值守运行或“任意命令均不询问”的模式**。完整理由和逐项语义见 [权限设计](docs/permissions.md)。

## 暂未实现

当前面向 `0.1.0` 的开发版本暂未实现 MCP、Memory、Plan、Skill、Subagent、独立 Artifact 系统、Git / 仓库级 Diff、Worktree、跨进程未完成 Turn 恢复、Event Journal、有损压缩或后台任务恢复。这描述的是当前状态，不代表这些能力都已承诺进入后续版本。进程重启后，未完成 Turn 会标记为 `interrupted`，不会自动重放工具。

以下是已知工程缺口，不代表已经承诺进入当前迭代：

- Rust → TypeScript Host Contract 仍由手写镜像维护，尚无自动生成和 Drift Check；
- Trace 标注仍只有 schema，Queue / 数据库 / Flush 的完整降级验收和丢弃计数出口尚未收口；
- `bash` 造成的文件副作用尚不能可靠对账；
- Live Provider Smoke Test 已有手工测试入口，但尚未自动化。

代码是“当前实际行为”的事实来源，`docs/` 描述目标设计；两者存在差距时，各设计文档的“尚未实施”小节会明确列出。

## 给贡献者

```mermaid
flowchart LR
    UI["Tauri Desktop<br/>React + TypeScript"] -->|"Command / Event"| Core["OpenWorkCore"]
    Core --> Registry["Session Registry"]
    Registry --> Actor["SessionActor"]
    Actor --> Chat["Chat State Actor"]
    Actor --> Model["Model Adapters<br/>HTTP + SSE"]
    Actor --> Tools["Tool Runtime<br/>Permission + Workspace"]
    Core --> DB[("PostgreSQL")]
    Actor -. "best-effort" .-> Trace["Trace Recorder"]
    Trace --> DB
```

| 路径 | 职责 |
|---|---|
| `desktop/` | Tauri 2 / React 桌面客户端；只适配 Command / Event，不复制后端状态机 |
| `crates/openwork-core/` | 唯一运行时入口：Session Runtime、Agent Loop、Compaction、Storage、Trace |
| `crates/openwork-agent/` | Agent 的静态定义：System Prompt、工具集和限制 |
| `crates/openwork-chat-state/` | Conversation 的唯一写者 |
| `crates/openwork-models/` | 模型协议、Provider Adapter、HTTP / SSE 和错误分类 |
| `crates/openwork-tools/` | 工具契约、权限策略、路径安全、文件与进程后端 |
| `docs/` | 权威设计文档，一个功能一篇 |

依赖保持单向：`openwork-models` 位于底层，`openwork-core` 组合运行时，Tauri 位于最外层。三条核心不变量是：一个活动 Session 只有一个 `SessionActor`、Agent Loop 只有 `session/run_loop.rs` 一处、Trace 失败不得改变业务结果。完整约束见 [架构文档](docs/architecture.md)。

验证命令：

```bash
# 仓库根目录
cargo test
cargo clippy --all-targets --all-features
cargo fmt

# desktop/
pnpm test
pnpm build
```

PostgreSQL 集成测试需要显式提供测试数据库，否则相关测试会提前返回：

```bash
TEST_DATABASE_URL=postgres://openwork:openwork@localhost:5432/openwork \
  cargo test -p openwork-core
```

文档入口：[索引](docs/README.md) · [架构](docs/architecture.md) · [Session 运行时](docs/session-runtime.md) · [上下文窗口](docs/context-window.md) · [压缩](docs/compaction.md) · [Trace](docs/trace.md) · [工具](docs/tools.md) · [权限](docs/permissions.md) · [数据模型](docs/data-model.md) · [Desktop](docs/desktop.md) · [本地 PostgreSQL](docs/local-postgres.md)

## 许可证

OpenWork 以 [Apache License 2.0](LICENSE) 开源。

---

发现问题或希望讨论设计时，请提交 [GitHub Issue](https://github.com/Aaron-Ben/OpenWork/issues)。
