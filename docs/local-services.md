# 本地服务

Server 把持久的事实存在 PostgreSQL，把会过期的协调数据存在 Redis。`compose.yaml` 定义这两个服务：

| 服务 | 容器 | 端口 | 数据 |
|---|---|---|---|
| `postgres` | `crew-postgres` | 5432 | 数据卷 `crew-postgres-data` |
| `redis` | `crew-redis` | 6379 | 没有数据卷，重启后清空 |

## 配置

Server 从根目录的 `.env` 读取两个地址。复制开发模板：

```bash
cp .env.example .env
```

模板指向 Compose 中的服务。`TEST_` 开头的两个变量给集成测试与冒烟测试用，见 [testing.md](testing.md)：

```text
DATABASE_URL=postgres://crew:crew@localhost:5432/crew
REDIS_URL=redis://localhost:6379/0
TEST_DATABASE_URL=postgres://crew:crew@localhost:5432/crew
TEST_REDIS_URL=redis://localhost:6379/1
```

## 启动

在仓库根目录运行：

```bash
docker compose up -d --wait
```

`--wait` 等到两个服务的健康检查都通过才返回。Server 启动时检查两个连接，任一失败就报错退出。

## 查看

```bash
docker compose ps
docker compose logs postgres redis
docker exec crew-postgres psql -U crew -d crew -c "\dt"
docker exec crew-redis redis-cli ping
```

## 停止或重建

停止两个服务，保留 PostgreSQL 的数据卷：

```bash
docker compose stop
```

删除全部本地数据：

```bash
docker compose down -v
docker compose up -d --wait
```

`down -v` 会永久删除 PostgreSQL 的数据卷。
