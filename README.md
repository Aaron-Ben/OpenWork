# OpenWork

<p align="center">
  <img src="docs/assets/openwork-readme.png" alt="OpenWork" width="420">
</p>

OpenWork 是一个围绕 Rust workspace 和 Tauri 桌面客户端构建的 AI 应用基础项目。项目目前处于早期阶段，优先推进多厂商模型接入、统一消息类型和桌面端基础壳层。

English version: [README.en.md](README.en.md)

## 项目状态

已经完成的基础能力包括：

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

## 目录结构

```text
OpenWork/
  apps/
    desktop/                 # Tauri + React desktop app
  crates/
    openwork-protocol/       # Core AI types, message blocks, traits, errors
    openwork-providers/      # Provider adapters for OpenAI, Anthropic, Kimi, DeepSeek, Qwen
    openwork-runtime/        # Model registry and runtime coordination
    openwork-tools/          # Tool abstractions and built-in tools
  docs/
    ai-provider-integration-design.md
```

## Rust 模块

`openwork-protocol`

- 定义 `GenerateRequest`、`GenerateResponse`
- 定义 `EmbeddingRequest`、`EmbeddingResponse`
- 定义 `Message` 和 `ContentBlock`
- 定义 provider trait 和统一错误类型

`openwork-providers`

- `OpenAiProvider`
- `AnthropicProvider`
- `KimiProvider`
- `DeepSeekProvider`
- `QwenProvider`
- `OpenAiCompatibleChatProvider`

`openwork-runtime`

- `ModelRegistry`
- 模型 capability 校验
- 默认模型和 fallback chain

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
cd apps/desktop
pnpm tauri dev
```

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

当前 provider 通过环境变量或调用方传入 API key。建议使用以下变量名：

```bash
OPENAI_API_KEY=...
ANTHROPIC_API_KEY=...
KIMI_API_KEY=...
DEEPSEEK_API_KEY=...
DASHSCOPE_API_KEY=...
```

不要提交真实密钥。

## 设计文档

模型接入设计见：

```text
docs/ai-provider-integration-design.md
```

## 下一步

建议优先推进：

1. 完善 provider response 的 block 化解析。
2. 增加 streaming 事件模型。
3. 增加 credential 管理，但先避免过度抽象。
4. 为桌面客户端接入最小可用的模型调用界面。
