# Local PostgreSQL

Last reviewed: 2026-07-18

> Status: current development setup. At this time, OpenWork supports only PostgreSQL. SQLx migrations are the source of truth for the schema.

## Database ownership

The migration baseline is at:

```text
crates/openwork-core/migrations/<initial schema>.sql
```

SQLx records the applied versions and checksums in `_sqlx_migrations`. Do not edit a migration after SQLx applies it. For each later schema change, add a new migration file.

When the directory changes, `crates/openwork-core/build.rs` tells Cargo to rebuild the embedded migration set.

The clean baseline creates:

| Table | Purpose |
| --- | --- |
| `_sqlx_migrations` | SQLx migration version, checksum, status, and execution time |
| `sessions` | Session metadata |
| `turns` | Turn lifecycle and token/tool summaries |
| `messages` | Complete model conversation messages |
| `trace_spans` | Best-effort diagnostics for Model Calls and Tool Calls |

Migration `202610030001_models_from_config.sql` drops `provider_credentials` and `models`. Providers are in `~/.openwork/config.json`.

This baseline has no backfill for a legacy schema. It is for the current pre-production stage, when you can rebuild the development database.

## Configuration

The default local connection is:

```text
postgres://openwork:openwork@localhost:5432/openwork
```

Copy the development environment template:

```bash
cp .env.example .env
```

## Start and migrate

From the repository root, run:

```bash
docker compose up -d postgres
cargo run -p openwork-core --bin openwork-migrate
```

`OpenWorkCore::bootstrap` also applies pending migrations before it serves commands. You can use the migration binary to provision and diagnose the database without the desktop application.

You can safely run the migration command many times. SQLx applies only the pending versions.

## Create a new migration

Install `sqlx-cli`. Then run:

```bash
sqlx migrate add --source crates/openwork-core/migrations <description>
```

Use separate migration files for schema changes and data changes. During the pre-production stage, do a destructive reset only when you explicitly choose it. Application startup must not silently delete unknown data.

## Inspect

```bash
docker compose ps postgres
docker compose logs postgres
docker exec openwork-postgres psql -U openwork -d openwork -c "SELECT version, description, success FROM _sqlx_migrations ORDER BY version"
```

## Stop or reset

Stop PostgreSQL and keep the local volume:

```bash
docker compose stop postgres
```

Delete all local database contents and rebuild from the clean baseline:

```bash
docker compose down -v
docker compose up -d postgres
cargo run -p openwork-core --bin openwork-migrate
```

`down -v` permanently deletes the local sessions, traces, model settings, and encrypted provider API keys.
