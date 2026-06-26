# Local PostgreSQL

Last reviewed: 2026-06-24

OpenWork uses PostgreSQL as the target persistence database. For local development, run PostgreSQL through Docker Compose.

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
