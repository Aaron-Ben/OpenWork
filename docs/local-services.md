# 本地服务

Server 把数据存在 PostgreSQL。`compose.yaml` 定义两个服务：

| 服务 | 容器 | 端口 | 数据 | 谁用 |
|---|---|---|---|---|
| `postgres` | `crew-postgres` | 5432 | 数据卷 `crew-postgres-data` | Crew 与旧版 |
| `redis` | `crew-redis` | 6379 | 没有数据卷，重启后清空 | 只有旧版，属于 `legacy` profile，默认不启动，见 [legacy-rust.md](legacy-rust.md) |

## 配置

Server 从根目录的 `.env` 读取数据库地址。复制开发模板：

```bash
cp .env.example .env
```

模板指向 Compose 中的服务。`TEST_DATABASE_URL` 给集成测试与冒烟测试用，见 [testing.md](testing.md)。模板里的 `REDIS_URL` 与 `TEST_REDIS_URL` 只给旧版用：

```text
DATABASE_URL=postgres://crew:crew@localhost:5432/crew
TEST_DATABASE_URL=postgres://crew:crew@localhost:5432/crew
```

## 启动

在仓库根目录运行：

```bash
docker compose up -d --wait
```

`--wait` 等到 PostgreSQL 的健康检查通过才返回。Server 启动时连不上数据库就报错退出。

## 查看

```bash
docker compose ps
docker compose logs postgres
docker exec crew-postgres psql -U crew -d crew -c "\dt"
```

## 停止或重建

停止服务，保留 PostgreSQL 的数据卷：

```bash
docker compose stop
```

删除全部本地数据：

```bash
docker compose down -v
docker compose up -d --wait
```

`down -v` 会永久删除 PostgreSQL 的数据卷。
