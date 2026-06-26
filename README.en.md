# OpenWork

<p align="center">
  <img src="docs/assets/openwork-readme.png" alt="OpenWork" width="420">
</p>

OpenWork is an AI application foundation built around a Rust workspace and a Tauri desktop client. The project is currently in early development and focuses first on multi-provider model integration, unified message types, and a desktop shell.

中文版本: [README.md](README.md)

## Project Status

The current foundation includes:

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

## Structure

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

## Rust Crates

`openwork-protocol`

- Defines `GenerateRequest` and `GenerateResponse`
- Defines `EmbeddingRequest` and `EmbeddingResponse`
- Defines `Message` and `ContentBlock`
- Defines provider traits and normalized provider errors

`openwork-providers`

- `OpenAiProvider`
- `AnthropicProvider`
- `KimiProvider`
- `DeepSeekProvider`
- `QwenProvider`
- `OpenAiCompatibleChatProvider`

`openwork-runtime`

- `ModelRegistry`
- Model capability checks
- Default models and fallback chains

## Desktop App

OpenWork Desktop is the Tauri + React + TypeScript client for OpenWork.

Stack:

- Tauri 2
- React
- TypeScript
- Vite
- pnpm

## Requirements

- Rust stable
- Node.js
- pnpm
- Tauri system dependencies

Install pnpm if needed:

```bash
corepack enable
corepack prepare pnpm@latest --activate
```

## Install Dependencies

Desktop app:

```bash
cd apps/desktop
pnpm install
```

## Run Desktop App

> Desktop commands must run inside `apps/desktop`. The repository root only has `Cargo.toml` for the Rust workspace and does not have `package.json`, so running `pnpm tauri dev` from the root fails with `ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND`.

```bash
cd apps/desktop
pnpm tauri dev
```

## Build Desktop App

```bash
cd apps/desktop
pnpm tauri build
```

## Rust Checks

From the repository root:

```bash
cargo test
cargo clippy --all-targets --all-features
cargo fmt
```

## API Keys

Provider API keys are currently passed through environment variables or caller configuration. Suggested names:

```bash
OPENAI_API_KEY=...
ANTHROPIC_API_KEY=...
KIMI_API_KEY=...
DEEPSEEK_API_KEY=...
DASHSCOPE_API_KEY=...
```

Do not commit real secrets.

## Design Doc

See:

```text
docs/ai-provider-integration-design.md
```

## Next Steps

Recommended near-term work:

1. Complete block-based provider response parsing.
2. Add a streaming event model.
3. Add credential management without over-abstracting it too early.
4. Build a minimal desktop UI for model calls.
