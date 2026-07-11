# Local PostgreSQL

Last reviewed: 2026-07-11

> Status: current development setup only. 当前代码仍依赖 PostgreSQL，因此本页命令仍可用于本地开发；目标 Persistence 边界见 [OpenWork Core 架构蓝图](../plans/openwork-core-architecture-blueprint.md)。

OpenWork currently uses PostgreSQL for persistence. For local development, run PostgreSQL through Docker Compose.

当前没有 SQLite 或多数据库兼容层。`schema_migrations` 记录已执行 migration；所有 schema 由 `openwork-persistence` 的显式 migrator 统一管理：

| 表 | 用途 |
| --- | --- |
| `providers` | Provider 名称、端点、Driver、加密后的 API Key、启用/激活状态、软删除状态和受限 Adapter Options |
| `provider_models` | Provider 下可选模型、`lite/plus/pro` 分类、启用状态、UI 顺序和软删除状态 |
| `recorded_events` | append-only Durable Event Journal；保存 Thread、Turn 和 Message 事实 |

当前仍处于可清库的开发阶段，因此 Provider Registry 使用单一干净基线，不保留旧明文 `api_key`、`models_json` 或旧 `kind/extra_body_json` 兼容列。`api_key_encrypted` 保存 AES-256-GCM 版本化密文；Provider 与模型列表的新增、更新使用同一 PostgreSQL 事务。

两张表统一使用 `created_at`、`updated_at`、`is_deleted`、`deleted_at`。时间列为不携带时区的 `TIMESTAMP WITHOUT TIME ZONE`，但创建、更新和删除时间都通过 `CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'` 写入东八区本地时间；删除 Provider 时不会立即物理删除行，而是在同一事务内软删除 Provider 和子模型；普通 Repository 查询统一排除 `is_deleted = true` 的记录。

运行前必须提供独立于数据库的主密钥。本地开发先复制示例配置：

```bash
cp .env.example .env
openssl rand -base64 32
```

把第二条命令的输出写入根目录 `.env`，不要提交真实值：

```dotenv
OPENWORK_API_KEY_ENCRYPTION_KEY=<生成的值>
```

`pnpm tauri dev` 的 Debug 构建会自动查找并加载根目录 `.env`；Release 构建不会加载开发 `.env`，仍须由运行环境显式注入。同一数据库必须持续使用同一个主密钥，不能在每次启动时重新生成，否则已有 API Key 无法解密。当前不迁移旧明文开发数据；旧表存在时按下方命令删除本地 volume 后重建，并重新填写 Provider API Key。

该边界只解决数据库静态数据泄露；拥有应用进程和环境变量读取权限的攻击者仍可取得主密钥。

## Start

From the repository root:

```bash
docker compose up -d postgres
cargo run -p openwork-persistence --bin openwork-migrate
```

Desktop 启动不执行 migration。新数据库或 schema 更新后必须先运行第二条命令；否则启动会列出缺失表并提示 migration 命令。

当前 migration 会删除旧的 `sessions/messages/llm_events/tool_runs` 表。完成后数据库只包含 `schema_migrations/providers/provider_models/recorded_events`。

The local connection string is:

```text
postgres://openwork:openwork@localhost:5432/openwork
```

`.env` is ignored by git.

## Check

```bash
docker compose ps postgres
docker compose logs postgres
```

If `psql` is installed locally:

```bash
psql postgres://openwork:openwork@localhost:5432/openwork
```

## Stop

Stop the container while preserving data:

```bash
docker compose stop postgres
```

Remove the container and local database volume:

```bash
docker compose down -v
```

Use `down -v` only when local database contents can be deleted.
