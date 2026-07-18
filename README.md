# OpenWork

<p align="center">
  <img src="docs/assets/openwork-readme.png" alt="OpenWork" width="420">
</p>

OpenWork 是一个以 Rust 实现的本地 Agent 工作台实验项目。当前代码具备多厂商模型调用、Session 级 Agent Loop、工具权限、PostgreSQL 持久化、Trace 和 Tauri Desktop。

English version: [README.en.md](README.en.md)

## 项目状态

已经完成的基础能力包括：

- Rust workspace 基础结构
- Tauri + React + TypeScript 桌面客户端骨架
- OpenAI、Anthropic、Kimi、DeepSeek、Qwen/DashScope 与 GLM Adapter
- 厂商无关的 `ModelRequest`、`ModelResponse`、`ModelEvent`、`ModelError` 与 `ModelPort`
- 可区分限流与额度耗尽的错误映射，以及流式输出感知的 Transport Retry
- Agent 多步工具调用、审批、取消和 doom-loop 检测
- `SessionActor` 持有 Model → Tool/Permission → Model 的唯一执行链
- Chat State Actor 串行维护模型 Conversation
- PostgreSQL 中的 Provider、Model、Session、Turn、Message 与 Trace 直接持久化
- SQLx 单一 migration 基线与独立迁移命令
- PostgreSQL Provider API Key 加密存储
- per-session Runtime Store、进程级 Event Bridge 和设置内运行记录

还没有完成：

- Rust → TypeScript Host Contract 生成与 Drift Check（当前保持手写 Bridge DTO）
- Trace 关闭入口，以及 Queue/数据库失败不影响 Turn 的完整降级验收
- 操作系统级 Sandbox 与可靠副作用对账
- 自动化 live provider smoke test

跨进程恢复未完成 Turn、Event Journal、Checkpoint、Memory、MCP、Plan、Skill、Git/Diff 和 Worktree 不属于当前 V1 范围。

## 目录结构

```text
OpenWork/
  apps/
    desktop/                 # Tauri + React desktop app
  crates/
    openwork-core/           # Core facade, Session Actor, PostgreSQL storage and Trace
    openwork-agent/          # Agent definition and system prompt
    openwork-chat-state/     # Conversation single-writer actor
    openwork-models/         # Model contracts, provider adapters and transport
    openwork-tools/          # Tool catalog, permissions and built-in execution
  docs/
    README.md                 # 文档索引
    local-postgres.md         # 本地 PostgreSQL 与 SQLx migration
    redesign/                 # 当前权威架构与重构状态
```

## Rust 模块

`openwork-core`

- `OpenWorkCore` 是唯一进程内入口，拥有 Provider Repository、凭证解析和 Session Registry
- `SessionActor` 负责 Model → Tool/Permission → Model 循环、取消和终态
- PostgreSQL SQLx 基线保存 Provider、Model、Session、Turn、Message 与 Trace
- Tauri 直接管理一个 `OpenWorkCore` State，只做 Command/Event 与安全错误映射

`openwork-models`

- 定义 `Message`、`ContentBlock`、`ModelPort` 和流事件
- 实现 OpenAI、Anthropic、DeepSeek、Kimi、Qwen、GLM Adapter
- 统一 HTTP/SSE Transport、厂商错误分类和重试

`openwork-agent` / `openwork-chat-state`

- Agent crate 只定义 Agent、System Prompt 和静态工具集合
- Chat State Actor 串行修改 Conversation，为模型请求提供一致快照

`openwork-tools`

- 持有工具定义、Schema、权限策略、工作目录上下文和内置文件/进程执行
- 当前不包含操作系统级 sandbox；Permission 不能绕过 `ToolContext` 的路径/进程约束
- 模型始终由用户显式选择 `providerId + model`；不提供自动选模或跨模型 Fallback

## 桌面端

OpenWork Desktop 是 OpenWork 的 Tauri + React + TypeScript 客户端。

技术栈：

- Tauri 2
- React
- TypeScript
- Vite
- pnpm

## 环境要求

- Rust stable
- Node.js
- pnpm
- Tauri 依赖环境

如果没有 pnpm：

```bash
corepack enable
corepack prepare pnpm@latest --activate
```

## 安装依赖

桌面端：

```bash
cd apps/desktop
pnpm install
```

## 启动桌面客户端

> 桌面端命令必须在 `apps/desktop` 目录下执行。项目根目录只有 `Cargo.toml`（Rust workspace），没有 `package.json`，在根目录运行 `pnpm tauri dev` 会报 `ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND`。

```bash
docker compose up -d postgres
cargo run -p openwork-core --bin openwork-migrate
cd apps/desktop
pnpm tauri dev
```

`OpenWorkCore::bootstrap` 会应用待执行 migration；独立迁移命令用于部署和数据库诊断。

## 构建桌面客户端

```bash
cd apps/desktop
pnpm tauri build
```

## Rust 测试和检查

在项目根目录运行：

```bash
cargo test
cargo clippy --all-targets --all-features
cargo fmt
```

## API Key

桌面端把用户填写的 API Key 加密后保存到 PostgreSQL `provider_credentials.api_key_encrypted`。`openwork-core` 的凭证存储使用 AES-256-GCM、随机 Nonce 和版本化 envelope，并使用 Provider ID 作为认证附加数据。

主密钥必须通过 `OPENWORK_API_KEY_ENCRYPTION_KEY` 提供，值为标准 Base64 编码的 32 字节随机数据，不能写入数据库或提交到 Git：

```bash
openssl rand -base64 32
```

本地开发时只生成一次，把结果填写到仓库根目录且已被 Git 忽略的 `.env`：

```dotenv
OPENWORK_API_KEY_ENCRYPTION_KEY=<上一步生成的值>
```

`pnpm tauri dev` 的 Debug 构建会自动加载根目录 `.env`；Release 构建不会读取开发 `.env`，仍须由部署环境注入。只要继续使用同一个数据库，就不能重新生成这个主密钥，否则已有 Provider API Key 将无法解密。

该设计保护数据库文件、备份或 SQL 导出泄露场景；如果攻击者同时控制应用进程并能读取环境变量，则仍可取得主密钥和解密后的 API Key。

当前使用开发期干净 schema，不迁移旧明文；已有开发库需要按 `docs/local-postgres.md` 重建 Provider 表并重新填写 API Key。底层 Adapter 仍保留从调用方或环境变量构造配置的入口。常用变量名：

```bash
OPENWORK_API_KEY_ENCRYPTION_KEY=...
OPENAI_API_KEY=...
ANTHROPIC_API_KEY=...
KIMI_API_KEY=...
DEEPSEEK_API_KEY=...
DASHSCOPE_API_KEY=...
```

不要提交真实密钥。

## 设计文档

- [文档索引](docs/README.md)
- [Runtime 重构设计](docs/redesign/README.md)
- [本地 PostgreSQL 与 SQLx Migration](docs/local-postgres.md)

## 下一步

建议优先推进：

1. 补齐取消、doom loop、Trace 降级和启动中断语义的自动化验收。
2. 决定何时恢复 Rust → TypeScript Host Contract 生成工作。
3. 为 Trace 增加可关闭配置与完整降级测试，或正式调整完成标准。
