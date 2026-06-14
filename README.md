# Anvil

Anvil is an AI application foundation built around a Rust workspace and a Tauri desktop client. The project is currently in early development and focuses first on model provider integration, unified message types, and a desktop shell.

中文说明见下方；English follows.

## 中文

### 项目状态

Anvil 目前处于早期阶段。已经完成的基础能力包括：

- Rust workspace 基础结构
- Tauri + React + TypeScript 桌面客户端骨架
- 多厂商大模型 provider 初版
- OpenAI、Anthropic、Kimi、DeepSeek、Qwen/DashScope 的接入骨架
- 文本生成、文本 embedding、多模态 message block 的基础建模
- Model Registry 基础能力

还没有完成：

- 生产级 streaming
- tool calling 完整闭环
- credential 管理
- 前端真实业务界面
- live API smoke test

### 目录结构

```text
Anvil/
  apps/
    desktop/              # Tauri + React desktop app
  crates/
    anvil-core/           # Core AI types, message blocks, traits, errors
    anvil-providers/      # Provider adapters for OpenAI, Anthropic, Kimi, DeepSeek, Qwen
    anvil-runtime/        # Model registry and runtime coordination
    anvil-tools/          # Future tool abstractions
  docs/
    ai-provider-integration-design.md
```

### Rust 模块

`anvil-core`

- 定义 `GenerateRequest`、`GenerateResponse`
- 定义 `EmbeddingRequest`、`EmbeddingResponse`
- 定义 `Message` 和 `ContentBlock`
- 定义 provider trait 和统一错误类型

`anvil-providers`

- `OpenAiProvider`
- `AnthropicProvider`
- `KimiProvider`
- `DeepSeekProvider`
- `QwenProvider`
- `OpenAiCompatibleChatProvider`

`anvil-runtime`

- `ModelRegistry`
- 模型 capability 校验
- 默认模型和 fallback chain

### 环境要求

- Rust stable
- Node.js
- pnpm
- Tauri 依赖环境

如果没有 pnpm：

```bash
corepack enable
corepack prepare pnpm@latest --activate
```

### 安装依赖

桌面端：

```bash
cd apps/desktop
pnpm install
```

### 启动桌面客户端

> 桌面端命令必须在 `apps/desktop` 目录下执行。项目根目录只有 `Cargo.toml`（Rust workspace），没有 `package.json`，在根目录运行 `pnpm tauri dev` 会报 `ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND`。

```bash
cd apps/desktop
pnpm tauri dev
```

### Rust 测试

在项目根目录运行：

```bash
cargo test
```

静态检查：

```bash
cargo clippy --all-targets --all-features
```

格式化：

```bash
cargo fmt
```

### API Key

当前 provider 通过环境变量或调用方传入 API key。建议使用以下变量名：

```bash
OPENAI_API_KEY=...
ANTHROPIC_API_KEY=...
KIMI_API_KEY=...
DEEPSEEK_API_KEY=...
DASHSCOPE_API_KEY=...
```

不要提交真实密钥。

### 设计文档

模型接入设计见：

```text
docs/ai-provider-integration-design.md
```

### 下一步

建议优先推进：

1. 完善 provider response 的 block 化解析。
2. 增加 streaming 事件模型。
3. 增加 credential 管理，但先避免过度抽象。
4. 为桌面客户端接入最小可用的模型调用界面。

## English

### Project Status

Anvil is in early development. The current foundation includes:

- Rust workspace structure
- Tauri + React + TypeScript desktop shell
- Initial multi-provider model integration
- Provider adapters for OpenAI, Anthropic, Kimi, DeepSeek, and Qwen/DashScope
- Basic text generation, text embedding, and multimodal message block modeling
- Basic model registry support

Not yet complete:

- Production-ready streaming
- Full tool calling loop
- Credential management
- Real desktop application workflows
- Live API smoke tests

### Structure

```text
Anvil/
  apps/
    desktop/              # Tauri + React desktop app
  crates/
    anvil-core/           # Core AI types, message blocks, traits, errors
    anvil-providers/      # Provider adapters
    anvil-runtime/        # Model registry and runtime coordination
    anvil-tools/          # Future tool abstractions
  docs/
    ai-provider-integration-design.md
```

### Rust Crates

`anvil-core`

- `GenerateRequest`, `GenerateResponse`
- `EmbeddingRequest`, `EmbeddingResponse`
- `Message` and `ContentBlock`
- Provider traits and normalized provider errors

`anvil-providers`

- `OpenAiProvider`
- `AnthropicProvider`
- `KimiProvider`
- `DeepSeekProvider`
- `QwenProvider`
- `OpenAiCompatibleChatProvider`

`anvil-runtime`

- `ModelRegistry`
- Capability checks
- Default models and fallback chains

### Requirements

- Rust stable
- Node.js
- pnpm
- Tauri system dependencies

Install pnpm if needed:

```bash
corepack enable
corepack prepare pnpm@latest --activate
```

### Install Desktop Dependencies

```bash
cd apps/desktop
pnpm install
```

### Run Desktop App

> Desktop commands must run inside `apps/desktop`. The repo root only has `Cargo.toml` (Rust workspace) with no `package.json`, so running `pnpm tauri dev` from the root fails with `ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND`.

```bash
cd apps/desktop
pnpm tauri dev
```

### Rust Checks

From the repository root:

```bash
cargo test
cargo clippy --all-targets --all-features
cargo fmt
```

### API Keys

Provider API keys are currently passed through environment variables or caller configuration. Suggested names:

```bash
OPENAI_API_KEY=...
ANTHROPIC_API_KEY=...
KIMI_API_KEY=...
DEEPSEEK_API_KEY=...
DASHSCOPE_API_KEY=...
```

Do not commit real secrets.

### Design Doc

See:

```text
docs/ai-provider-integration-design.md
```

### Next Steps

Recommended near-term work:

1. Complete block-based provider response parsing.
2. Add a streaming event model.
3. Add credential management without over-abstracting it too early.
4. Build a minimal desktop UI for model calls.
