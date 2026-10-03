# Agent Note: 去掉 Postgres 与 Redis，改用文件与嵌入式 SQLite

Status: proposed

## 问题

OpenWork 是单机桌面应用（用户决定，2026-10-03），但运行时要两个外部服务：

- Postgres：工作台 12 张表，协作 17 张表。用户要按 [local-postgres.md](../../../../docs/local-postgres.md) 或 `compose.yaml` 启动它。
- Redis：协作 Server 的唤醒、seen、HELD 与限速。

这带来几个后果：

- 安装与启动的门槛高。
- 测试依赖 `TEST_DATABASE_URL` 与 `TEST_REDIS_URL`，没有设置时静默返回，只靠 `scripts/check.sh` 强制设置兜底。
- 东八区墙上时间的约定扩散到 22 个 Rust 文件与全部迁移。

Raft 是多租户 SaaS，服务端用 Postgres（190 张表），但 Redis 只服务多副本部署（`packages/server/src/replicaRouter.ts`），本机 daemon 全部用文件。单机产品不需要这两个服务。

## 提议

### 存储去向

| 现在 | 去向 |
|---|---|
| `sessions`、`turns`、`messages`、`conversation_compactions`、`turn_plans` | 会话事件日志，见 [会话事件日志提议](../architecture/2026-10-03-session-event-log.md) |
| `models`、`provider_credentials`、`skill_status` | `~/.openwork/config.json`，明文保存，文件权限 `0600` |
| `trace_spans`、`trace_payloads`、`trace_span_payloads`、`trace_annotations` | `~/.openwork/trace.sqlite` |
| 协作的 17 张 `collab_*` 表 | `~/.openwork/collab/collab.sqlite`，只有协作 Server 进程读写 |

### Redis 去向

协作 Server 是单个进程。[collaboration.md](../../../../docs/subsystems/collaboration.md) 已规定 Redis 只存可过期、可重复、可丢失的状态，所以这些状态可以放进进程内存：

| Redis key | 用途 | 去向 |
|---|---|---|
| `openwork:wake:*`、`openwork:message.new` | 发布唤醒与新消息 | `tokio::sync::broadcast` |
| `openwork:wake-claim:*` | 唤醒去重 | 进程内带过期时间的集合 |
| `openwork:seen:*` | 已看到的位置 | 进程内映射；需要持久时写入 `collab.sqlite` |
| `openwork:hold:*` | HELD token | 进程内带过期时间的映射 |
| `openwork:turn-rate:*`、`openwork:agenda-rate:*` | 限速 | 进程内令牌桶 |
| `openwork:agenda-declines:*`、`openwork:agenda-nudge:*` | Agenda 冷却 | 进程内映射 |

### SQL 改写

代码与迁移中有这些 Postgres 专有写法：

| 写法 | 数量 | 改法 |
|---|---|---|
| `AT TIME ZONE`、`to_char` | 144 | 时间改存 Unix 毫秒整数，删除 |
| `jsonb` / `JSONB` | 41 | `TEXT` 存 JSON，用 SQLite 的 JSON 函数 |
| `ON CONFLICT` | 25 | SQLite 支持相同语法 |
| `FOR UPDATE` | 23 | 用 `BEGIN IMMEDIATE` 串行写事务 |
| `RETURNING` | 14 | SQLite 支持 |
| `INTERVAL` | 9 | 在 Rust 中计算时刻 |
| `DEFERRABLE` 唯一约束 | 3 | SQLite 不支持。看板改为留空档的位置值，移动时只改被移动的卡片 |
| `infinity`（一直静音） | 3 | 用 `NULL` 加一个布尔列表示 |
| `pg_advisory` 锁 | 2 | 进程内互斥锁 |

### 凭证

不加密（用户决定，2026-10-03）。API key 与 Provider 配置一起明文写入 `~/.openwork/config.json`，只靠文件权限 `0600` 保护。删除 `openwork-credentials` 的 AES-GCM 加密与加密密钥环境变量（`API_KEY_ENCRYPTION_KEY_ENV`）。

### 旧数据

不迁移现有的 Postgres 数据（用户决定，2026-10-03）。切换后，旧会话、Provider 配置与协作房间都不可见。

### 开发环境

删除 `compose.yaml`、`docs/local-postgres.md`、`TEST_DATABASE_URL` 与 `TEST_REDIS_URL`。测试使用临时目录中的 SQLite 文件，不再有静默跳过的分支。`scripts/check.sh` 不再要求任何环境变量。

## 考虑过的方案

**只为协作保留 Postgres。** 没有采用：协作也只在本机运行，保留一个数据库服务只为一个进程使用，门槛不变。

**嵌入式 Postgres（例如 `postgresql_embedded` crate）。** 没有采用：首次运行要下载并解压 Postgres 二进制，体积与启动时间都高于 SQLite，也仍然要管理一个子进程。

**保留 Redis。** 没有采用：Redis 的跨进程发布订阅只在多副本部署时有用。协作 Server 是单个进程，进程内结构语义相同，还少一个服务。

## 验收条件

- 安装后直接运行 Desktop，不需要启动任何数据库或缓存服务。
- `scripts/check.sh` 不需要环境变量；仓库中没有因环境缺失而静默返回的测试。
- 代码中没有 `sqlx` 的 `postgres` feature，也没有 `redis` 与 `aes-gcm` 依赖。
- 协作的验收条目（[collaboration.md](../../../../docs/subsystems/collaboration.md) 验收一节）在 SQLite 与进程内状态下全部通过，包括并发发言只成功一次、卡片领取先到先得。
- `docs/local-postgres.md` 删除，`docs/data-model.md` 改写为 SQLite 与文件布局。

## 风险

- API key 明文存放。能读用户主目录的程序都能读到它，这与 pi、Claude Code 等工具的做法一致。
- 协作的并发规则现在依赖行锁（`FOR UPDATE`）。改为串行写事务后，要用测试重新确认每一条规则，例如逐字重复检查、卡片领取。
- SQLite 同一时刻只允许一个写事务。协作 Server 写入频率低，预计不是瓶颈，需要在多 Agent 场景下测量。
- 进程内状态在协作 Server 重启时丢失。按现有规定这些状态本来就可以丢，但要确认丢失后的行为，例如 HELD token 失效后，Agent 直接重发即可。
