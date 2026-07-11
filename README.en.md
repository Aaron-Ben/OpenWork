# OpenWork

<p align="center">
  <img src="docs/assets/openwork-readme.png" alt="OpenWork" width="420">
</p>

OpenWork is a local agent workbench experiment implemented in Rust. The current code supports multi-provider model calls, an agent tool loop, approvals, and PostgreSQL persistence; the target architecture is a recoverable and verifiable Durable Agent Harness.

中文版本: [README.md](README.md)

## Project Status

The current foundation includes:

- Rust workspace structure
- Tauri + React + TypeScript desktop shell
- Adapters for OpenAI, Anthropic, Kimi, DeepSeek, Qwen/DashScope, GLM, and custom OpenAI-compatible endpoints
- Vendor-neutral `ModelRequest`, `ModelResponse`, `ModelEvent`, `ModelError`, and `ModelPort`
- Error mapping that separates rate limits from exhausted quota, plus stream-aware transport retry
- Multi-step agent tool calls, approvals, cancellation, and doom-loop detection
- PostgreSQL provider repository, sessions, messages, and basic LLM event persistence
- Tauri + React + TypeScript desktop client

Not yet complete:

- Durable Turn journal, crash recovery, and idempotent projections
- OS-level sandboxing and reliable side-effect reconciliation
- Target implementations for context compaction, planning, memory, MCP, and skills
- System keychain/secret store (API keys are currently stored in PostgreSQL as plaintext)
- Automated live-provider smoke tests

## Structure

```text
OpenWork/
  apps/
    desktop/                 # Tauri + React desktop app
  crates/
    openwork-protocol/       # Core AI types, message blocks, traits, errors
    openwork-providers/      # Pure model HTTP/SSE adapters, errors, retry
    openwork-persistence/    # PostgreSQL provider repository and migrations
    openwork-agent/          # Current agent loop
    openwork-runtime/        # Desktop-facing runtime composition
    openwork-session/        # PostgreSQL sessions, messages, and trace events
    openwork-tools/          # Tool abstractions and built-in tools
  docs/
    model-provider-v1-design.md
```

## Rust Crates

`openwork-protocol`

- Defines `ModelRequest`, `ModelResponse`, and `ModelEvent`
- Defines `Message` and `ContentBlock`
- Defines `ModelPort`, `ProviderRepository`, and normalized model errors

`openwork-providers`

- `OpenAiProvider`
- `AnthropicProvider`
- `KimiProvider`
- `DeepSeekProvider`
- `QwenProvider`
- `OpenAiCompatibleChatProvider`
- Normalized vendor error mapping and `RetryingModelPort`

`openwork-persistence`

- `PostgresProviderRepository`
- `providers` / `provider_models` migrations
- Transactional provider-and-model writes

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

The desktop currently persists user-entered API keys with provider configuration in PostgreSQL. This is known technical debt; a system keychain has not been integrated. Low-level adapters also accept caller-provided or environment-backed configuration. Common variable names:

```bash
OPENAI_API_KEY=...
ANTHROPIC_API_KEY=...
KIMI_API_KEY=...
DEEPSEEK_API_KEY=...
DASHSCOPE_API_KEY=...
```

Do not commit real secrets.

## Design Docs

- [Documentation index](docs/README.md)
- [OpenWork Core architecture blueprint](plans/openwork-core-architecture-blueprint.md)
- [Model Provider V1 design](docs/model-provider-v1-design.md)

## Next Steps

Recommended near-term work:

1. Establish the Golden Case and a minimal eval harness.
2. Define the Protocol Foundation and the runtime semantics for planning, capabilities, retries, and approvals.
3. Build replayable persistence and the Capabilities/Execution contracts.
4. Move the current agent loop into recoverable and verifiable `openwork-core` durable turns.
