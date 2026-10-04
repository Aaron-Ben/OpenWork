# Local services

The Server stores durable facts in PostgreSQL and expiring coordination data in Redis. `compose.yaml` defines both services:

| Service | Container | Port | Data |
|---|---|---|---|
| `postgres` | `crew-postgres` | 5432 | Volume `crew-postgres-data` |
| `redis` | `crew-redis` | 6379 | No volume. A restart clears it. |

## Configuration

The Server reads both addresses from the root `.env`. Copy the development template:

```bash
cp .env.example .env
```

The template points at the Compose services:

```text
DATABASE_URL=postgres://crew:crew@localhost:5432/crew
REDIS_URL=redis://localhost:6379/0
```

## Start

From the repository root, run:

```bash
docker compose up -d --wait
```

`--wait` returns after both health checks pass. The Server checks both connections at startup and exits with an error when either one fails.

## Inspect

```bash
docker compose ps
docker compose logs postgres redis
docker exec crew-postgres psql -U crew -d crew -c "\dt"
docker exec crew-redis redis-cli ping
```

## Stop or reset

Stop both services and keep the PostgreSQL volume:

```bash
docker compose stop
```

Delete all local data:

```bash
docker compose down -v
docker compose up -d --wait
```

`down -v` permanently deletes the PostgreSQL volume.
