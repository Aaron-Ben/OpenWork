# Local PostgreSQL

Last reviewed: 2026-07-18

> Status: current development setup. OpenWork currently supports PostgreSQL only and uses SQLx migrations as the schema source of truth.

## Database ownership

The migration baseline lives at:

```text
crates/openwork-core/migrations/<initial schema>.sql
```

SQLx records applied versions and checksums in `_sqlx_migrations`. Do not edit a migration after it has been applied; add a new migration file for every later schema change.

`crates/openwork-core/build.rs` tells Cargo to rebuild the embedded migration set whenever the directory changes.

The clean baseline creates:

| Table | Purpose |
| --- | --- |
| `_sqlx_migrations` | SQLx migration version, checksum, status, and execution time |
| `provider_credentials` | Provider metadata and encrypted API keys |
| `models` | Selectable model endpoints and provider credential references |
| `sessions` | Session metadata |
| `turns` | Turn lifecycle and token/tool summaries |
| `messages` | Complete model conversation messages |
| `trace_spans` | Best-effort Model Call and Tool Call diagnostics |

There is no legacy schema backfill in this baseline. It is intended for the current pre-production stage where the development database can be rebuilt.

## Configuration

The default local connection is:

```text
postgres://openwork:openwork@localhost:5432/openwork
```

Provider API keys are encrypted independently of the database connection. Copy the development environment template and create a stable 32-byte key:

```bash
cp .env.example .env
openssl rand -base64 32
```

Write the generated value to `.env`:

```dotenv
OPENWORK_API_KEY_ENCRYPTION_KEY=<generated value>
```

Do not commit the real value. Changing this key makes existing encrypted provider credentials unreadable.

## Start and migrate

From the repository root:

```bash
docker compose up -d postgres
cargo run -p openwork-core --bin openwork-migrate
```

`OpenWorkCore::bootstrap` also applies pending migrations before it serves commands. The explicit migration binary remains useful for provisioning and diagnosing the database independently of the desktop application.

Running the migration command repeatedly is safe; SQLx only applies pending versions.

## Create a new migration

With `sqlx-cli` installed:

```bash
sqlx migrate add --source crates/openwork-core/migrations <description>
```

Schema and data changes should use separate migration files. During the pre-production phase a destructive reset is allowed only when explicitly chosen; application startup must not silently delete unknown data.

## Inspect

```bash
docker compose ps postgres
docker compose logs postgres
docker exec openwork-postgres psql -U openwork -d openwork -c "SELECT version, description, success FROM _sqlx_migrations ORDER BY version"
```

## Stop or reset

Stop PostgreSQL while preserving the local volume:

```bash
docker compose stop postgres
```

Delete all local database contents and rebuild from the clean baseline:

```bash
docker compose down -v
docker compose up -d postgres
cargo run -p openwork-core --bin openwork-migrate
```

`down -v` permanently removes local sessions, traces, model settings, and encrypted provider API keys.
