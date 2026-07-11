# Event Journal 与会话持久化重构设计

Last reviewed: 2026-07-11

> Status: implemented for Thread/Turn/Message persistence; Durable Action/Approval remains staged. 本文细化 [OpenWork Core 架构蓝图](./openwork-core-architecture-blueprint.md) 的 S2 Persistence，不改变蓝图中的模块所有权。

## 1. 为什么要重构

重构前，会话持久化由三块遗留基础设施共同完成：

```text
openwork-session
  -> sessions / messages / llm_events / tool_runs
  -> 连接数据库并自行执行迁移

openwork-database
  -> PgPool、migration runner、通用 PgCrud/PgFilterQuery

openwork-db-macros
  -> 为 openwork-session 生成 PgEntity/PgCrud metadata
```

桌面端还会独立创建 `PostgresPersistence` 和 `SessionStore`，产生两个连接池和两个迁移入口，数据库生命周期由 Tauri 启动过程隐式控制。

更重要的是，四张旧表没有形成可靠的事实日志：

- `messages` 是当前唯一可靠的聊天恢复来源，但只保存最终消息，无法回答工具和审批过程发生了什么。
- `llm_events` 保存的是 UI payload；每个事件通过 detached `tokio::spawn` best-effort 写入，错误被忽略，顺序也不可靠。
- `tool_runs` 只有表结构，没有运行时写入路径。
- `sessions` 混合了 UI 会话视图和未来 Thread 聚合的事实来源。

因此这次重构不是简单把四张表换成一张表，而是建立以下分层：

```text
Recorded facts       recorded_events（唯一事实来源、append-only）
        |
        v
Query projections    threads / messages / action_runs / approvals（按需要逐步增加）
        |
        v
Live UI stream       text_delta / reasoning_delta / tool_call_delta（不承诺回放）
```

## 2. 目标与不做事项

### 2.1 本次目标

1. 在 `openwork-protocol` 定义稳定的 `EventJournal` Port、事件 Envelope 和 Expected Version。
2. 在 `openwork-persistence` 实现 PostgreSQL append-only Journal、连接池和迁移生命周期。
3. 使用 `recorded_events` 记录 Thread、Turn、Action、Approval 的可回放事实。
4. 将 Session/Message DTO、Repository 和内存投影迁入 `openwork-persistence`。
5. 删除 `openwork-session`、`openwork-db-macros` 和 `openwork-database`，由 Persistence 内部基础设施统一承担连接池与 migration 生命周期。

### 2.2 本次不做

- 不记录每个 token、reasoning delta 或 tool input delta；这些是高频 UI 流。
- 不实现自动模型路由。
- 不在 V1 预先加入 `correlation_id`、`causation_id`、`metadata_json` 等尚无消费方的字段。
- 不在 Journal 表上增加 `updated_at`、`is_deleted`、`deleted_at`；事实通过追加纠正事件表达，不原地更新或软删除。
- 不对旧 Session/Message 数据做 backfill；当前处于可清库开发阶段。

## 3. 术语与边界

### 3.1 Session、Thread 与 Turn

- `session`：当前前端 API 使用的兼容名称。
- `thread`：领域中的长期对话聚合，可包含多个 Turn。
- `turn`：一次用户输入到本次 Agent 结束、失败或取消的执行过程。

前端命令暂时保留 `session_*` 名称，但 Persistence 中的新事实使用 `thread`/`turn` 语义。不能因为 UI 仍叫 Session，就继续让 `sessions` 表充当事实来源。

### 3.2 Recorded Event 与 Live Event

Recorded Event 是恢复、审计和投影必须依赖的事实，例如：

- 用户消息已接受；
- 工具调用已请求；
- 审批已请求、已允许或已拒绝；
- 工具执行已开始、已成功、已失败或结果未知；
- Turn 已完成、失败或取消。

Live Event 只服务当前 UI，例如 `text_delta`、`reasoning_delta`、`tool_call_delta`。丢失单个 Live Event 不应改变系统恢复出的业务状态。

## 4. `recorded_events` 表

V1 只新增一张事实表：

```sql
CREATE TABLE recorded_events (
    global_position BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    event_id TEXT NOT NULL UNIQUE,
    aggregate_type TEXT NOT NULL,
    aggregate_id TEXT NOT NULL,
    aggregate_version BIGINT NOT NULL,
    event_type TEXT NOT NULL,
    event_version INTEGER NOT NULL DEFAULT 1,
    payload_json JSONB NOT NULL,
    occurred_at TIMESTAMP WITHOUT TIME ZONE NOT NULL,
    recorded_at TIMESTAMP WITHOUT TIME ZONE NOT NULL
        DEFAULT (CURRENT_TIMESTAMP AT TIME ZONE 'Asia/Shanghai'),

    CONSTRAINT recorded_events_event_id_not_blank
        CHECK (btrim(event_id) <> ''),
    CONSTRAINT recorded_events_aggregate_type_not_blank
        CHECK (btrim(aggregate_type) <> ''),
    CONSTRAINT recorded_events_aggregate_id_not_blank
        CHECK (btrim(aggregate_id) <> ''),
    CONSTRAINT recorded_events_aggregate_type_valid
        CHECK (aggregate_type IN ('thread', 'turn')),
    CONSTRAINT recorded_events_aggregate_version_positive
        CHECK (aggregate_version > 0),
    CONSTRAINT recorded_events_event_type_not_blank
        CHECK (btrim(event_type) <> ''),
    CONSTRAINT recorded_events_event_version_positive
        CHECK (event_version > 0),
    CONSTRAINT recorded_events_payload_is_object
        CHECK (jsonb_typeof(payload_json) = 'object'),
    CONSTRAINT recorded_events_aggregate_version_unique
        UNIQUE (aggregate_type, aggregate_id, aggregate_version)
);
```

索引：

```sql
CREATE INDEX idx_recorded_events_aggregate
    ON recorded_events(aggregate_type, aggregate_id, aggregate_version);

CREATE INDEX idx_recorded_events_type_position
    ON recorded_events(event_type, global_position);
```

### 4.1 每个字段的作用

| 字段 | 作用 | 为什么不能由其他字段替代 |
| --- | --- | --- |
| `global_position` | 全库事实的单调游标，Projector 从“上次处理到哪里”继续 | `aggregate_version` 只在单个聚合内递增，不能给全库投影排序 |
| `event_id` | 事件的稳定身份，用于幂等写入和跨边界引用 | `global_position` 由数据库生成，重试前调用方无法持有它 |
| `aggregate_type` | 事件所属业务对象种类，V1 为 `thread` 或 `turn` | 避免含糊的 `stream_kind`；它描述的是领域聚合类型，不是传输流格式 |
| `aggregate_id` | 具体 ThreadId 或 TurnId | 不同聚合分别并发演进，不能只靠全局位置定位 |
| `aggregate_version` | 同一聚合内的严格顺序，也是乐观并发版本 | 防止两个写者都基于旧状态追加冲突事实 |
| `event_type` | 已发生事实的稳定名称 | payload 只保存数据，不应靠猜测 JSON 形状判断语义 |
| `event_version` | 单个 `event_type` 的 payload schema 版本 | 数据库 migration 版本不能表达历史事件 JSON 的兼容版本 |
| `payload_json` | 事件特有数据 | 不把所有事件的可选字段摊平到一张超宽表 |
| `occurred_at` | 业务事实发生时间，由应用写入 | 执行结束后补写结果时，它可能早于数据库接收时间 |
| `recorded_at` | 数据库真正接受事实的时间 | 用于诊断写入延迟和审计入库顺序 |

`occurred_at`、`recorded_at` 均使用无时区字段，值按东八区墙上时间写入和展示。这延续当前 Provider 表的约定。跨时区产品化前必须重新评估该决定；V1 不混用 `TIMESTAMPTZ` 与无时区时间。

### 4.2 为什么不叫 `stream_kind`

`stream_kind` 容易同时被理解为 SSE stream、UI stream、模型流或 Event Store stream。数据库字段实际回答的是“这个事件属于哪一种领域聚合”，因此使用 `aggregate_type` 更准确。

V1 只允许：

- `thread`：标题、创建、归档/删除等跨 Turn 的对话事实。
- `turn`：模型尝试、消息、Action、Approval 和 Turn 终态。

Action 与 Approval 暂不各自创建独立 aggregate；它们在 payload 中携带 `action_run_id` / `approval_id`，并保持在所属 Turn 的严格顺序中。

## 5. 最小事件目录

事件名称使用过去式，表达已经发生的事实：

### 5.1 Thread aggregate

| event_type | 关键 payload |
| --- | --- |
| `thread_created` | `title`、`provider_id`、`model`、`working_dir` |
| `thread_title_changed` | `title` |
| `thread_deleted` | 空对象 `{}` |

### 5.2 Turn aggregate

| event_type | 关键 payload |
| --- | --- |
| `turn_started` | `thread_id`、`provider_id`、`model` |
| `user_message_recorded` | `message_id`、`content` |
| `model_attempt_started` | `model_attempt_id`、`step` |
| `model_attempt_completed` | `model_attempt_id`、`finish_reason`、`usage` |
| `model_attempt_failed` | `model_attempt_id`、稳定错误分类 |
| `assistant_message_recorded` | `message_id`、`content` |
| `action_requested` | `action_run_id`、`name`、`input` |
| `approval_requested` | `approval_id`、`action_run_id`、`reason` |
| `approval_resolved` | `approval_id`、`action_run_id`、`resolution` |
| `action_started` | `action_run_id` |
| `action_completed` | `action_run_id`、`observation` 或 `artifact_id` |
| `action_failed` | `action_run_id`、稳定错误分类 |
| `action_denied` | `action_run_id`、`reason` |
| `action_cancelled` | `action_run_id` |
| `action_outcome_unknown` | `action_run_id`、`reason` |
| `turn_completed` | 最终 message/结果引用 |
| `turn_failed` | 稳定错误分类 |
| `turn_cancelled` | 空对象 `{}` |

大体积工具输出后续写入 Artifact Store；Journal 只保存摘要和 `artifact_id`，避免 `payload_json` 无限膨胀。

## 6. Append、并发与幂等

`EventJournal::append` 必须接收 Expected Version：

```text
NoStream        仅允许当前聚合不存在，用于第一个事件
Exact(n)        仅允许当前聚合最新版本为 n
```

PostgreSQL 实现的单批 append 算法：

1. 开启事务。
2. 对 `(aggregate_type, aggregate_id)` 获取 transaction-scoped advisory lock。
3. 查询当前 `MAX(aggregate_version)`。
4. 校验 Expected Version；不一致则整体失败，不写任何事件。
5. 按输入顺序分配连续 `aggregate_version` 并写入。
6. 提交事务。

`UNIQUE(event_id)` 负责重试幂等身份；`UNIQUE(aggregate_type, aggregate_id, aggregate_version)` 是最终并发保护。不能用 `SELECT MAX(...) + 1` 而不加聚合锁，否则并发写者会竞争同一版本。

## 7. Projection 与为什么以后仍可能增加表

Journal 可以恢复一切，但不适合每次 UI 列表都扫描全量 JSON。后续表不是新的事实来源，而是可删除、可重建的查询投影：

- `threads`：会话列表和标题。
- `messages`：聊天回放与上下文读取。
- `action_runs`：工具调用审计。
- `approvals`：待审批和历史决策。

是否增加某张投影表，只由真实查询需求和性能证据决定。投影必须保存 `last_global_position` 或使用独立 projector checkpoint，并能从 `recorded_events` 清空重建。

这符合三大范式：Journal 每行只表达一个事件；Provider 配置仍由 `providers` / `provider_models` 管理；投影中的 Thread、Message、Action、Approval 各自表示单一实体，不把重复 Provider 或 Message 字段塞入事件主表列。

## 8. Crate 重构结果

目标依赖方向：

```text
openwork-protocol
  <- openwork-core
  <- openwork-app

openwork-protocol
  <- openwork-persistence

apps/desktop -> openwork-app + openwork-persistence
```

### 8.1 `openwork-protocol`

新增稳定类型和 Port，不包含 SQLx、PostgreSQL Row、Tauri payload：

- `EventId`
- `AggregateType`
- `RecordedEventV1`
- `NewRecordedEventV1`
- `ExpectedVersion`
- `EventJournal` / `EventJournalError`

### 8.2 `openwork-persistence`

最终统一拥有：

- 单一 PostgreSQL 连接池；
- 全库 migration runner；
- Provider Repository；
- Event Journal；
- 后续 Projector/Repository/Artifact 实现。

### 8.3 `openwork-session`

已删除。Desktop 兼容 DTO、Session Repository 和当前内存投影已经迁入 `openwork-persistence::session`。

### 8.4 `openwork-database`

已删除。`DatabaseConfig` 是 Persistence 的公开连接配置；`Database`、连接池和 migration runner 已迁入 `openwork-persistence::postgres` 私有模块。

### 8.5 `openwork-db-macros`

已删除。Journal 使用显式 SQL，更适合表达 Expected Version、advisory lock、幂等投影和批量事务。

## 9. 施工结果与后续阶段

### Phase A：Journal 基础设施（已完成）

- 增加 Protocol 合同和序列化测试。
- 增加 `recorded_events` migration、PostgreSQL Adapter 和并发/幂等测试。
- Persistence 提供单一连接池和显式 migration API。

### Phase B1：Thread/Turn/Message 直接切换（已完成）

- Thread 创建/改名/删除只写 Journal。
- Turn 开始前持久化 `turn_started` 和 `user_message_recorded`。
- Assistant/Tool Message 和 Turn 终态按批追加。
- UI delta 仍只走订阅，不写 Journal。
- Session/Message 查询通过内存重放 `recorded_events` 完成。
- forward migration 删除 `sessions/messages/llm_events/tool_runs`。
- 删除 `openwork-session` 和 `openwork-db-macros`。

由于用户明确接受丢弃开发数据，本阶段没有双写或 backfill。应用代码先切换到 Journal，随后由 migration DROP 旧表。

### Phase B2：Durable Action/Approval（待完成）

- Core 在副作用 Action 前持久化 intent，在执行后持久化 outcome。
- Approval request 必须在 UI 可见前持久化，resolution 必须在继续执行前持久化。
- Journal 写失败时，涉及副作用的 Turn 不得继续执行。
- 补齐 `outcome_unknown` 与 reconciliation。

### Phase C：持久化查询 Projection（按需）

- 当前内存投影在开发数据量下足够，不立即新增表。
- 出现可测量的全量回放性能问题后，再增加 Thread/Message/ActionRun/Approval 查询投影。
- Synthetic Event 必须能从零 Replay 并重建相同投影。
- 投影只由 `global_position` 推进，可删除重建。

已经执行的 forward migration 删除：

```text
tool_runs
llm_events
messages
sessions
```

`openwork-session`、`openwork-db-macros` 和 `openwork-database` 均已删除；数据库基础设施只存在于 Persistence 内部。

## 10. 迁移生命周期

当前 `connect()` 只建立连接并检查必需表，migration 是显式操作。Desktop 启动不再静默建表。

本地开发命令：

```bash
cargo run -p openwork-persistence --bin openwork-migrate
cd apps/desktop && pnpm tauri dev
```

显式 migrator 会执行 `drop_legacy_session_tables`。最终必需表检查只包含 `schema_migrations/providers/provider_models/recorded_events`。

## 11. 验收测试

- Contract：Envelope serde round-trip，ID/版本语义稳定。
- Migration：只有 `recorded_events` 是新事实表；时间类型为 `TIMESTAMP WITHOUT TIME ZONE`。
- Append：`NoStream`、`Exact(n)`、批量连续版本。
- Conflict：错误 Expected Version 不产生部分写入。
- Idempotency：相同 `event_id` 重试不会产生第二条事实。
- Replay：按 `aggregate_version` 重建单聚合；按 `global_position` 驱动 Projector。
- Golden Case：用户消息 -> 模型 -> 审批 -> Action -> Observation -> 最终回复，Recorded trace 完整且顺序稳定。

## 12. Decision Log

| 决策 | 结果 | 原因 |
| --- | --- | --- |
| 直接保留 `llm_events` 作为 Journal | 拒绝 | 它保存 UI payload，写入 best-effort 且没有 Expected Version |
| 只执行 DROP、不修改读写代码 | 拒绝 | 会让 Desktop 立即因缺表失败 |
| Journal 读写切换后直接 DROP 四张旧表 | 接受 | 用户确认开发数据可丢，且 Session/Message 已能从 Journal 回放 |
| V1 同时新增所有 Projection 表 | 拒绝 | 尚无投影读路径和性能证据，先建立缺失的事实来源 |
| 新表叫 `events` | 拒绝 | 语义过宽，无法区分 UI/Telemetry 事件 |
| 字段叫 `stream_kind` | 拒绝 | 容易和 SSE/UI stream 混淆，改为领域语义明确的 `aggregate_type` |
| Journal 使用软删除字段 | 拒绝 | append-only 事实不能原地修改或删除 |
| 使用一张 `recorded_events` 作为 V1 事实源 | 接受 | 最小化 schema，同时支持顺序、并发、回放和投影 |
