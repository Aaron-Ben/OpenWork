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
- Adapters for OpenAI, Anthropic, Kimi, DeepSeek, Qwen/DashScope, and GLM
- Vendor-neutral `ModelRequest`, `ModelResponse`, `ModelEvent`, `ModelError`, and `ModelPort`
- Error mapping that separates rate limits from exhausted quota, plus stream-aware transport retry
- Multi-step agent tool calls, approvals, cancellation, and doom-loop detection
- PostgreSQL provider repository and provider-model configuration persistence
- Append-only Event Journal, Journal-backed threads/turns/messages, and explicit migrations
- Encrypted PostgreSQL storage for provider API keys
- Tauri + React + TypeScript desktop client

Not yet complete:

- Complete Durable Turn journal writes, crash recovery, and idempotent projections
- OS-level sandboxing and reliable side-effect reconciliation
- Target implementations for context compaction, planning, memory, MCP, and skills
- Automated live-provider smoke tests

## Structure

```text
OpenWork/
  apps/
    desktop/                 # Tauri + React desktop app
  crates/
    openwork-protocol/       # Stable model/capability/approval/journal contracts
    openwork-core/           # Turn control loop and approval state ownership
    openwork-app/            # Application API, supervisor, composition
    openwork-providers/      # Pure model HTTP/SSE adapters, errors, retry
    openwork-persistence/    # PostgreSQL repositories, Journal, and migrations
    openwork-capabilities/   # Tool declarations and capability discovery
    openwork-execution/      # Schema validation and built-in action handlers
    openwork-workspace/      # Project workspace, Git, and file-boundary primitives
  docs/
    model-provider-v1-design.md
```

## Rust Crates

`openwork-protocol`

- Defines `ModelRequest`, `ModelResponse`, and `ModelEvent`
- Defines `Message` and `ContentBlock`
- Defines `CapabilitySpec`, `ActionRequest`, and `Observation`
- Defines recorded events, Expected Version, and `EventJournal`
- Defines `ModelPort`, `ProviderRepository`, `CapabilityResolverPort`, and `ExecutionPort`

`openwork-providers`

- `OpenAiProvider`
- `AnthropicProvider`
- `KimiProvider`
- `DeepSeekProvider`
- `QwenProvider`
- `GlmProvider`
- Normalized vendor error mapping and `RetryingModelPort`

`openwork-persistence`

- `PostgresProviderRepository`
- `PostgresEventJournal`
- `providers` / `provider_models` migrations
- Append-only `recorded_events` migration and explicit migrator
- Session/Turn-event-backed session/message creation, replay, rename, and deletion
- Transactional provider-and-model writes
- AES-256-GCM encryption for provider API keys stored in PostgreSQL

`openwork-capabilities`

- Owns built-in action names, descriptions, input schemas, and declaration-side risk hints
- Implements discovery through `CapabilityCatalog` without performing filesystem or process I/O

`openwork-execution`

- Organizes real handlers under `actions/filesystem` and `actions/process`
- Owns argument validation, path permissions, cancellation, timeouts, output truncation, and `Observation` normalization
- Does not yet provide an OS-level sandbox; `risk_hint` contributes to approval reasons but cannot replace final argument-level risk evaluation

`openwork-core` / `openwork-app`

- Core owns the Turn loop and approval pause/resume state
- `OpenWorkApplication` is the single composition root for providers, the capability catalog, execution, Core, and Journal-backed sessions
- Desktop accesses application capabilities only through the provider/session/turn application services
- The user always selects `providerId + model` explicitly; there is no automatic model selection or cross-model fallback

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
cargo run -p openwork-persistence --bin openwork-migrate
cd apps/desktop
pnpm tauri dev
```

Run the migration command explicitly from the repository root. Desktop startup only checks the schema and never creates tables automatically.

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

The desktop still stores user-entered API keys in PostgreSQL, but the `providers` table only stores the `api_key_encrypted` ciphertext. `openwork-persistence` uses AES-256-GCM with a random nonce in a versioned envelope and binds the ciphertext to the provider ID as authenticated associated data.

The master key must be supplied through `OPENWORK_API_KEY_ENCRYPTION_KEY` as standard Base64 encoding of 32 random bytes. It must not be stored in PostgreSQL or committed to Git:

```bash
openssl rand -base64 32
```

For local development, generate the value once and put it in the repository-root `.env`, which is ignored by Git:

```dotenv
OPENWORK_API_KEY_ENCRYPTION_KEY=<value generated above>
```

The Debug build used by `pnpm tauri dev` loads the root `.env` automatically. Release builds do not load the development `.env` and still require deployment-time injection. Do not regenerate the master key while retaining the same database, or existing provider API keys will no longer decrypt.

This protects database files, backups, and SQL dumps. It does not protect secrets from an attacker who controls the application process and can read its environment and decrypted memory.

The project currently uses a clean development schema and does not migrate old plaintext values. Existing development databases must rebuild the provider tables and re-enter their API keys. Low-level adapters still accept caller-provided or environment-backed configuration. Common variable names:

```bash
OPENWORK_API_KEY_ENCRYPTION_KEY=...
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
