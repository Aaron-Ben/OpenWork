# Local PostgreSQL

The Collaboration Server stores durable facts in PostgreSQL. All its tables use the `collab_` prefix.

## Database ownership

The migration files are in `crates/openwork-collab/migrations/`. The ordered list that the Server applies is `MIGRATIONS` in `crates/openwork-collab/src/server/migration.rs`.

When the Server connects (`server/db.rs`), it applies each pending migration in one transaction. It records the applied versions in `collab_schema_migrations`. Do not edit a migration after a database applies it. For each schema change, add a new file and append it to `MIGRATIONS`.

A database that an older OpenWork build used can also contain tables without the `collab_` prefix, such as `sessions` and `_sqlx_migrations`. The current code does not read them. To remove them, reset the database (see below).

## Configuration

The default local connection is:

```text
postgres://openwork:openwork@localhost:5432/openwork
```

Copy the development environment template:

```bash
cp .env.example .env
```

## Start

From the repository root, run:

```bash
docker compose up -d postgres
```

The Desktop starts the Collaboration Server, and the Server applies the migrations. No separate migration command exists.

## Inspect

```bash
docker compose ps postgres
docker compose logs postgres
docker exec openwork-postgres psql -U openwork -d openwork -c "SELECT version, description FROM collab_schema_migrations ORDER BY version"
```

## Stop or reset

Stop PostgreSQL and keep the local volume:

```bash
docker compose stop postgres
```

Delete all local database contents:

```bash
docker compose down -v
docker compose up -d postgres
```

`down -v` permanently deletes the local agents, rooms, messages, boards, and runs. The next Server start creates the schema again.
