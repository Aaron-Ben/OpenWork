# OpenWork Crate Restructure Plan

Last reviewed: 2026-06-24

## 1. 目标

本计划描述 OpenWork `crates/` 的目标组织方式。当前项目允许破坏性重构，因此优先追求清晰职责、单向依赖和后续可维护性，而不是维持现有 crate 名称兼容。

核心决策：

- 只使用 PostgreSQL 作为持久化数据库，不保留 SQLite 兼容层。
- 存储层采用 PostgreSQL-first 设计，参考 `metastable/crates/database` 与 `metastable/crates/db-macros` 的 macro-driven entity 思路。
- 不让业务 store 直接散落手写 SQL；SQL 生成、row mapping、基础 CRUD、schema metadata 由 database crate 和 macro 统一承载。
- `openwork-runtime` 不再承载全部业务逻辑，而是作为面向宿主应用的组合 service 层。

## 2. 目标目录

```text
crates/
  openwork-protocol/
  openwork-db-macros/
  openwork-database/
  openwork-session/
  openwork-provider/
  openwork-agent/
  openwork-tools/
  openwork-permissions/
  openwork-workspace/
  openwork-runtime/
```

## 3. Crate 职责

### 3.1 `openwork-protocol`

底层协议与共享类型。其他 crate 可以依赖它，但它不依赖业务 crate。

职责：

- AI message / content block 类型。
- tool call / tool result 类型。
- provider request / response / stream event 类型。
- agent event 类型。
- approval / permission 的外部 DTO。
- session / trace / tool run 的跨层 DTO。
- 统一错误码或可序列化错误 envelope。

当前状态：

- `openwork-protocol/src/ai/types.rs`
- `openwork-protocol/src/ai/error.rs`
- `openwork-protocol/src/ai/traits.rs`

### 3.2 `openwork-db-macros`

PostgreSQL entity derive 宏。参考 metastable 的 `SqlxObject`，但命名和边界应更贴近 OpenWork。

建议宏：

```rust
#[derive(PgEntity)]
#[table_name = "sessions"]
pub struct SessionRecord {
    #[primary_key]
    pub id: Uuid,

    #[indexed]
    pub provider_id: Uuid,

    pub title: String,
    pub model: String,
    pub working_dir: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

宏生成：

- `PgSchema` 实现。
- row struct / `FromRow` 映射。
- `insert_sql` / `update_sql` / `delete_sql`。
- 基础 CRUD trait 实现。
- index metadata。
- typed column enum。
- typed query builder helper。

不建议直接照搬：

- 全局 `connect()` 函数。
- 隐式全局 pool。
- 自由字符串 filter 作为主 API。
- 自动危险 migration，例如默认 drop column 或强制 alter type。

### 3.3 `openwork-database`

PostgreSQL 基础设施层。它是唯一直接关心 `PgPool`、transaction、migration、SQL 渲染和 sqlx 的基础 crate。

建议结构：

```text
src/
  lib.rs
  config.rs
  pool.rs
  transaction.rs
  entity.rs
  query.rs
  migration.rs
  error.rs
```

职责：

- `Database` 显式连接对象。
- `DatabaseConfig`，包含 database url、pool size、timeout。
- `PgEntity` / `PgSchema` / `PgCrud` trait。
- typed `QueryCriteria`、`OrderDirection`、cursor pagination。
- migration runner。
- transaction helper。
- PostgreSQL 类型映射。

连接方式应显式：

```rust
pub struct Database {
    pool: PgPool,
}

impl Database {
    pub async fn connect(config: DatabaseConfig) -> Result<Self, DatabaseError>;
    pub fn pool(&self) -> &PgPool;
    pub async fn migrate(&self) -> Result<(), DatabaseError>;
}
```

### 3.4 `openwork-session`

会话领域与持久化 repository。它不直接持有 Tauri 状态，也不应该把 SQL 直接写在业务方法里。

建议结构：

```text
src/
  lib.rs
  records/
    session.rs
    message.rs
    message_part.rs
    llm_event.rs
    tool_run.rs
    worktree_snapshot.rs
  repository.rs
  error.rs
```

职责：

- 定义 session 相关 record。
- 定义 session repository trait。
- 提供 PostgreSQL repository 实现，基于 `openwork-database`。
- 管理会话、消息、LLM events、tool runs。

核心表：

```text
sessions
messages
llm_events
tool_runs
```

`tool_runs` 应作为一等表，而不是从 `llm_events` 反推。
工作区文件变更暂不建表持久化，优先依赖 Git diff/restore。

### 3.5 `openwork-provider`

模型 provider 子系统。

建议结构：

```text
src/
  traits.rs
  error.rs
  registry.rs
  config.rs
  records.rs
  repository.rs
  adapters/
    openai.rs
    anthropic.rs
    openai_compatible.rs
    kimi.rs
    deepseek.rs
    qwen.rs
    glm.rs
  codecs/
    openai_chat.rs
    openai_responses.rs
    anthropic_messages.rs
```

职责：

- provider trait。
- provider config record 和 repository。
- provider factory。
- model registry。
- 不同厂商 adapter。
- provider-specific request/response codec。

`openwork-provider` 可以依赖 `openwork-protocol` 和 `openwork-database`，但不能依赖 `openwork-agent` 或 `openwork-runtime`。

### 3.6 `openwork-agent`

Agent loop 与模型/工具调度。

职责：

- 多步 agent loop。
- provider streaming 转 agent event。
- tool call 收集和执行调度。
- doom-loop 检测。
- cancellation。
- max steps。
- system prompt / context assembly 的核心流程。

不应负责：

- Tauri event emit。
- PostgreSQL 连接管理。
- provider 配置文件读取。
- worktree snapshot 具体落库。

### 3.7 `openwork-tools`

工具抽象与内置工具。

建议结构：

```text
src/
  lib.rs
  definition.rs
  registry.rs
  context.rs
  output.rs
  builtin/
    fs/
    search/
    shell/
    patch/
```

职责：

- `Tool` trait。
- `ToolRegistry`。
- `ToolContext`。
- 内置工具：read、write、edit、list、grep、glob、bash、后续 apply_patch。
- 工具结果类型。

工具不做审批决策。能进入 `execute` 代表上层已经允许执行。

### 3.8 `openwork-permissions`

权限与审批模型。

职责：

- `PermissionProfile`。
- filesystem policy。
- network policy。
- approval policy。
- approval request / decision。
- 风险摘要。
- 后续 sandbox executor 的抽象边界。

拆出该 crate 的原因：

- 权限不是工具本身。
- 审批不是工具本身。
- 后续 sandbox、危险命令识别、自动审批规则会快速膨胀。

### 3.9 `openwork-workspace`

工作区、git、snapshot 和 revert。

职责：

- working directory 解析。
- git status。
- changed file capture。
- diff snapshot。
- revert snapshot。
- path safety。
- 后续文件搜索或 patch 应用的共享基础能力。

当前 Tauri command 中的 snapshot/revert/git 逻辑应迁入此 crate。

### 3.10 `openwork-runtime`

组合 service 层。它面向宿主应用，组合其他 crate，而不是承载所有核心逻辑。

建议结构：

```text
src/
  lib.rs
  chat_service.rs
  provider_service.rs
  session_service.rs
  approval_service.rs
  event_mapper.rs
```

职责：

- 组装 database、provider、agent、tools、session、workspace。
- 给 Tauri 或未来 server 暴露稳定 service API。
- 把 `AgentEvent` 映射成宿主层 event payload。
- 管理单次 chat request 生命周期。

`apps/desktop/src-tauri` 应尽量变薄，只保留 command 参数解析、state 注入、event emit 和错误映射。

## 4. 依赖方向

```text
openwork-protocol
  ↑
openwork-database ← openwork-db-macros
  ↑
openwork-session       openwork-provider       openwork-permissions
      ↑                  ↑                    ↑
      └──────────── openwork-agent ← openwork-tools ┘
                ↑              ↑
            openwork-workspace ───┘
                ↑
          openwork-runtime
                ↑
      apps/desktop/src-tauri
```

约束：

- `openwork-protocol` 不依赖任何业务 crate。
- `openwork-database` 不依赖 session/provider/agent/runtime。
- `openwork-provider` 不依赖 agent/runtime。
- `openwork-tools` 不依赖 agent/runtime。
- `openwork-agent` 不依赖 Tauri。
- `openwork-runtime` 可以依赖多个业务 crate，但业务 crate 不反向依赖 runtime。

## 5. PostgreSQL Schema 原则

建议默认类型：

| 用途 | PostgreSQL 类型 | Rust 类型 |
| --- | --- | --- |
| 主键 | `uuid` | `Uuid` |
| 时间 | `timestamptz` | `DateTime<Utc>` |
| 普通文本 | `text` | `String` |
| 结构化 payload | `jsonb` | `serde_json::Value` 或 typed JSON wrapper |
| 文件内容 | `bytea` | `Vec<u8>` |
| 标志位 | `boolean` | `bool` |

索引原则：

- `sessions(updated_at)` 用于会话列表排序。
- `messages(session_id, seq)` 用于恢复上下文。
- `llm_events(session_id, seq)` 和 `llm_events(request_id, seq)` 用于 trace。
- `tool_runs(session_id, request_id)` 用于工具审计。
- JSONB 只有当需要按内容查询时再增加 GIN index。

## 6. Query API 原则

metastable 的 `QueryCriteria::add_filter("id", "=", value)` 很灵活，但 OpenWork 不应把自由字符串作为主要 API。

推荐生成 typed column：

```rust
SessionRecord::query()
    .eq(SessionColumn::ProviderId, provider_id)
    .order_by(SessionColumn::UpdatedAt, OrderDirection::Desc)
    .limit(50)
```

如果确实需要 raw filter，应限制在 database crate 内部或标记为 escape hatch。

## 7. Migration 原则

宏可以生成初始 schema 和安全增量，但 migration 不能完全隐式。

允许自动化：

- create table if not exists。
- create index if not exists。
- add nullable column。
- add non-null column with explicit default。

需要显式 migration：

- drop column。
- rename column。
- type change。
- data backfill。
- constraint 收紧。
- 大表索引策略。

不建议默认开启：

- 自动 drop column。
- 自动 alter column type。
- 自动删除未知 index。

## 8. 迁移顺序

### Phase 1: Protocol 与命名边界

- 新增 `openwork-protocol`。
- 将原 AI 类型迁入。
- 调整 provider/tools/runtime/session 引用。
- 删除旧兼容层。

### Phase 2: PostgreSQL 基础设施

- 新增 `openwork-database`。
- 新增 `Database`、`DatabaseConfig`、pool、error、transaction。
- 新增最小 `PgSchema` / `PgCrud` trait。
- 新增 `openwork-db-macros` 的第一版，只生成 metadata 和 row mapping。

### Phase 3: Session PostgreSQL 化

- 删除 `rusqlite` 路径。
- 重建 session records。
- 新增 `tool_runs`。
- 将 `SessionStore` 改为 PostgreSQL repository。
- 更新 Tauri 启动流程，改为创建 `Database` 和 repository。

### Phase 4: Provider 配置入库

- 将 provider config store 从文件/内存实现迁入 PostgreSQL。
- 新增 `providers` 和 `provider_models` 表。
- `openwork-provider` 负责 provider repository 和 factory。

### Phase 5: Agent 与 Runtime 拆分

- 新增 `openwork-agent`。
- 将 `openwork-runtime/src/router.rs` 中 agent loop 迁入。
- `openwork-runtime` 改为 service composition。
- Tauri command 调用 runtime service。

### Phase 6: Workspace 抽离

- 新增 `openwork-workspace`。
- 迁移 Tauri 中的 git status、snapshot、revert、path safety。
- session repository 只负责落库，不负责抓取 worktree 内容。

## 9. 当前落地状态

截至 2026-06-25：

- `openwork-protocol` 已新增，承载 AI message、provider trait、stream event、tool call/result 等协议类型。
- 旧兼容层已删除。
- `openwork-database` 已新增，当前提供 PostgreSQL 配置、`PgPool` 连接基础设施和 migration runner。
- `openwork-db-macros` 已新增，当前提供 `PgEntity` derive，生成表名、字段、主键和索引 metadata。
- `openwork-session` 已切换到 PostgreSQL，SQLite 路径已移除。
- `openwork-providers` 的 provider config store 已切换到 PostgreSQL。
- `openwork-session` / `openwork-providers` 的 schema 已迁入 records + explicit migrations，store 不再自行维护 `run_schema()`。
- `openwork-agent` 已新增，agent loop 已从 `openwork-runtime` 迁入。
- `openwork-permissions` 已新增，审批策略和权限模型已从 `openwork-tools` 迁入。
- `openwork-workspace` 已新增，Tauri 中的 git status、snapshot diff、revert、path safety 已迁入该 crate。
- `openwork-runtime` 当前保留 model registry，并 re-export agent / permissions 的宿主 API。
- `openwork-db-macros` 还没有生成 row mapping、typed column enum、query builder；session/provider store 的 CRUD SQL 仍需逐步迁入 typed query。

## 10. 当前文档影响

完成上述重构后，需要同步更新：

- `docs/architecture-overview.md`
- `docs/agent-runtime-and-tool-flow.md`
- `docs/session-persistence-and-tracing.md`
- `docs/permissions-and-approvals.md`
- `README.md`

在重构未完成前，这份文档中的目标结构不完全代表当前实现；以上“当前落地状态”优先反映实际代码。
