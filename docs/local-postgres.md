# Local PostgreSQL

Last reviewed: 2026-07-11

> Status: current development setup only. 当前代码仍依赖 PostgreSQL，因此本页命令仍可用于本地开发；目标 Persistence 边界见 [OpenWork Core 架构蓝图](../plans/openwork-core-architecture-blueprint.md)。

OpenWork currently uses PostgreSQL for persistence. For local development, run PostgreSQL through Docker Compose.

当前没有 SQLite 或多数据库兼容层。`schema_migrations` 记录已执行 migration；Provider 相关表由 `openwork-persistence` 管理：

| 表 | 用途 |
| --- | --- |
| `providers` | Provider 名称、端点、Driver、API Key、启用/激活状态和受限 Adapter Options |
| `provider_models` | Provider 下可选模型、`lite/plus/pro` 分类、启用状态和 UI 顺序 |

当前仍处于可清库的开发阶段，因此 Provider Registry 使用单一干净基线 migration，不保留 `models_json` 或旧 `kind/extra_body_json` 兼容列。Provider 与模型列表的新增、更新使用同一 PostgreSQL 事务。

## Start

From the repository root:

```bash
docker compose up -d postgres
```

The local connection string is:

```text
postgres://openwork:openwork@localhost:5432/openwork
```

Copy `.env.example` to `.env` when local commands need `DATABASE_URL`:

```bash
cp .env.example .env
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
