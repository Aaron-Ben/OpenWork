# Desktop Tauri 与 Application API 边界重构设计

> 文档地位更新（2026-07-16）：本文保留为 Desktop/Application 边界的历史实施记录，不再定义本轮目标分层。新的 Host、App 与 Runtime 所有权以 [`docs/redesign/01-project-structure.md`](../docs/redesign/01-project-structure.md) 为准。
>
> 历史快照状态：Phase A/B 当时已经实现，Phase C 当时仍是提案。
> Last reviewed: 2026-07-14.
> Parent blueprint: [OpenWork Core 架构蓝图](./openwork-core-architecture-blueprint.md) S7。

## 1. 背景

OpenWork 需要 Tauri 作为桌面宿主：React 运行在 WebView 中，Rust 持有 Agent、Provider、Persistence、Execution 和本地权限能力，Tauri 负责窗口、IPC、事件和打包。问题不在于使用 Tauri，而在于当前 `apps/desktop/src-tauri` 同时承担了宿主适配和 Application Composition 两类职责。

Phase A 重构前，`src-tauri/src/lib.rs` 直接创建并注册：

```text
PostgresPersistence
ProviderRepository
ProviderFactory
SessionStore
ChatRuntime
RequestCancelRegistry
```

重构前 `commands/provider.rs` 直接执行 Provider Repository CRUD 和连接测试，`commands/session.rs` 直接调用 `SessionStore`。这使 Desktop 知道数据库、Repository、Factory 和 Projection 的具体实现，与蓝图中“`openwork-app` 是唯一 Composition Root、Desktop 不绕过 App 访问内部 Store 或 Registry”的约束不一致。

## 2. 决策

保留 Tauri，但将 `apps/desktop/src-tauri` 收缩为 Desktop Host Adapter：

1. `openwork-app` 成为唯一 Application Composition Root。
2. Desktop 只管理一个 `OpenWorkApplication` 状态，不再分别管理 Repository、Store、Factory 和取消注册表。
3. Tauri Command 只负责 IPC 参数转换、调用 Application API 和返回 Desktop DTO。
4. Provider、Session、Turn、Approval 和 Cancel 用例全部由 `openwork-app` 提供显式 API。
5. Tauri Command、TypeScript API、Application Service 和 Journal aggregate 统一采用 Session 命名。
6. 不引入通用 Command Bus、反射式路由或新的网络服务；在第二个宿主出现前继续使用进程内显式 Rust API。

### 2.1 Phase A/B 当前实现结果

- `openwork-app::OpenWorkApplication` 已成为唯一 Composition Root。
- App 已按 Provider、Session 和 Turn 拆出三个显式 Application Service。
- Tauri setup 只注册一个 `OpenWorkApplication` State。
- Provider/Session/Chat Command 已不再引用 Repository、Store、Factory 或独立 Cancel Registry。
- Desktop 已删除对 `openwork-persistence`、`openwork-providers`、`openwork-workspace` 和 `openwork-protocol` 的直接 Cargo 依赖。
- 现有 Tauri Command 名、参数和成功响应 DTO 保持兼容；Phase B 有意把错误与 Live Event 升级为结构化合同。
- 新增源码结构测试锁定 App Root 和 Desktop 依赖方向。
- Application Service 已统一返回 `ApplicationError { code, message }`，Tauri 仅映射为同构 `CommandError`。
- UI 实时事件已改为 `TurnLiveEvent` tagged enum，TypeScript 使用同构 discriminated union。
- 前端错误解析与流事件归并已增加 Vitest 行为测试。

尚未完成：统一 shutdown、初始化失败页面、CSP 和 Opener 权限收紧。

目标依赖方向：

```text
React / TypeScript
  -> Tauri invoke / event
  -> apps/desktop/src-tauri
  -> openwork-app
       -> openwork-core
       -> openwork-providers
       -> openwork-persistence
       -> openwork-capabilities
       -> openwork-execution
       -> openwork-workspace
       -> openwork-protocol
```

## 3. 边界所有权

### 3.1 Tauri 当前与目标职责

- 创建窗口、加载 WebView 和桌面打包。
- 从宿主环境读取开发 `.env`，构造或传入应用启动配置。
- 注册 `#[tauri::command]`。
- 将 IPC DTO 映射为 Application Command/Query。
- 将 Application Live Event 发送到指定窗口或 App Handle。
- Phase C 接收应用退出事件并调用 Application shutdown。
- 当前管理插件 Capability；release CSP 和 Opener 权限收紧属于 Phase C。

### 3.2 Tauri 不负责

- 创建 `PostgresProviderRepository` 或 `SessionStore`。
- 决定 Provider RuntimeConfig 如何加载或测试。
- 执行 Session 业务规则和 Journal Projection。
- 维护 Turn、Approval 或 Cancel 的所有权状态。
- 构造 Capability Catalog、ExecutionService 或 Agent。
- 把底层 SQLx、加密或 Provider Adapter 错误直接暴露给前端。

### 3.3 `openwork-app` 负责

- 当前根据 `ApplicationConfig` 组合 Persistence、Providers、Capabilities、Execution 和 Core；Workspace 接线仍待对应阶段。
- 当前持有 Provider、Session 和 Turn 的 Application Service。
- 当前持有 TurnSupervisor 和取消生命周期。
- 当前提供显式 Command/Query 方法，并保留现有 Live Event callback 兼容入口。
- 当前已把底层错误映射为稳定的 Application Error Code，并把 Live Event 冻结为 tagged enum。
- Phase C 在关闭时取消活跃 Turn，并为后续后台任务提供统一 shutdown 边界。

### 3.4 其他模块保持原边界

- `openwork-protocol`：稳定领域类型、ID、Port 和 Recorded Event 合同，不包含 Tauri 类型。
- `openwork-core`：Turn 控制循环和审批暂停/恢复，不依赖 Tauri。
- `openwork-persistence`：PostgreSQL、Journal、Projection 和加密，不依赖 App/Desktop。
- `openwork-providers`：模型协议 Adapter、Transport 和 Retry，不包含 Provider 设置页面 DTO。

## 4. 重构前问题与目标映射

| 当前实现 | 问题 | 目标 |
| --- | --- | --- |
| Tauri setup 创建多个具体组件 | 出现第二个 Composition Root | `OpenWorkApplication::bootstrap` 统一组装 |
| `ProviderRepositoryState` | Desktop 直接持有 Repository | `ProviderApplicationService` |
| Command 直接持有 `SessionStore` | Desktop 知道 Persistence/Projection | `SessionApplicationService` |
| Command 直接调用 `ProviderFactory` | Provider 测试用例落在宿主层 | App 的 `test_provider` Command |
| `Result<T, String>` | 前端只能解析文案 | 稳定 `CommandError { code, message }` |
| 字符串事件 + 大量 `Option` | Rust 可构造无效 payload | tagged Application Live Event 枚举 |
| 独立 Cancel Registry | Turn 生命周期所有权分散 | Application 统一 register/cancel/shutdown |
| Rust 测试读取 TypeScript 源文件 | 测试跨越实现层 | Rust 合同测试和前端行为测试分离 |

## 5. Application API V1

### 5.1 应用入口

`openwork-app` 当前提供单一应用入口：

```rust
pub struct OpenWorkApplication {
    providers: ProviderApplicationService,
    sessions: SessionApplicationService,
    turns: TurnApplicationService,
}

impl OpenWorkApplication {
    pub async fn bootstrap(config: ApplicationConfig) -> Result<Self, ApplicationBootstrapError>;

    pub fn providers(&self) -> &ProviderApplicationService;
    pub fn sessions(&self) -> &SessionApplicationService;
    pub fn turns(&self) -> &TurnApplicationService;

}
```

这是 Phase A/B 当前实现的接口形状；`shutdown` 属于 Phase C，尚未加入。关键约束是具体 Adapter 的组装只出现一次，且 Desktop 不取得内部 Repository/Store getter。

`ApplicationConfig` 接收规范化配置，不依赖 Tauri 类型。开发模式下 `.env` 的加载仍可留在 Desktop Host，但数据库地址、连接池参数和凭据主密钥如何转成应用配置由 App 启动入口统一处理。

### 5.2 Provider Command/Query

```text
Query:
  list_providers
  list_provider_presets

Command:
  create_provider
  update_provider
  delete_provider
  activate_provider
  test_saved_provider
  test_provider_draft
```

Provider Preset 是产品级发现数据，可以作为 App Query 形状；它不进入 `openwork-providers` Adapter，也不进入稳定 Model Protocol。若以后某些 website/api-key URL 只服务 Desktop 展示，再把纯展示字段留在前端 DTO。

### 5.3 Session Command/Query

```text
Query:
  list_threads
  load_thread

Command:
  create_thread
  rename_thread
  delete_thread
```

现有 `session_list/session_create/session_load/session_rename/session_delete` Tauri Command 调用这些 API，并返回 Session DTO；各层不再保留 Thread 兼容命名。

### 5.4 Turn Command/Subscription

```text
Command:
  start_turn
  resolve_approval
  cancel_turn

Subscription:
  subscribe_turn_events
```

Turn ID 是运行时所有权标识；`request_id` 只作为当前 Desktop 兼容字段。Cancel 和 Approval 必须路由到持有该 Turn 的 Application Supervisor，Tauri 不自行维护第二份 Turn 状态。

## 6. Tauri Adapter 当前结构

```text
apps/desktop/src-tauri/src/
├── main.rs
├── lib.rs                 # Builder、插件，并直接注册 State<OpenWorkApplication>
├── error.rs               # ApplicationError -> CommandError
└── commands/
    ├── mod.rs
    ├── chat.rs
    ├── provider.rs
    └── session.rs
```

当前没有单独增加只有一行包装价值的 `state.rs`，也不引入 `transport/`、`controller/` 等抽象目录。只有当 State 生命周期或 Event/DTO 映射明显膨胀时再按真实职责拆分。

目标 Command 形状：

```rust
#[tauri::command]
async fn session_list(
    application: tauri::State<'_, OpenWorkApplication>,
) -> Result<Vec<SessionSummary>, CommandError> {
    application
        .sessions()
        .list()
        .await
        .map_err(CommandError::from)
}
```

Command 中禁止出现以下具体类型：

```text
PostgresPersistence
PostgresProviderRepository
ProviderRepositoryState
ProviderFactory
SessionStore
CapabilityCatalog
ExecutionService
```

## 7. 错误合同

Application Service 当前把 Repository、Journal、Core 和 Provider 调用错误映射为稳定的 `ApplicationError`；Desktop 再进行无损的宿主 DTO 映射：

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: CommandErrorCode,
    pub message: String,
}
```

V1 错误码至少覆盖：

```text
invalid_request
provider_not_found
session_not_found
turn_not_found
approval_not_found
database_unavailable
schema_not_ready
configuration_invalid
operation_conflict
model_request_failed
internal_error
```

`message` 用于展示，前端分支只依赖 `code`。TypeScript 的 `resolveCommandError` 保留合法结构化错误；只有旧字符串、JavaScript `Error` 或未知值才降级为 `internal_error`。SQL、数据库地址、API Key、加密材料和完整 Provider 错误 body 不进入 Desktop Error。

`provider_test` 中“远端模型明确返回认证、余额或协议失败”仍是正常的连接测试结果 `ProviderTestResult { success: false, message }`；Provider 不存在、Repository 不可用或请求缺少必要输入则是 `CommandError`。前者是被测试对象的业务结果，后者是命令本身未能成立。

## 8. Live Event 合同

当前 Application Live Event 使用公共信封加 serde tagged enum；公共路由字段只出现一次，每个变体只能携带自身所需字段：

```rust
pub struct TurnLiveEvent {
    pub request_id: String,
    pub session_id: String,
    #[serde(flatten)]
    pub kind: TurnLiveEventKind,
}

#[serde(
    tag = "event",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum TurnLiveEventKind {
    TextDelta { delta: String },
    ToolResult {
        tool_call_id: String,
        tool_name: String,
        output: String,
        is_error: bool,
    },
    ApprovalRequest {
        approval_id: String,
        tool_name: String,
        input: serde_json::Value,
    },
    Done,
    Error { message: String },
}
```

完整枚举还包含 step、LLM、text/reasoning block、tool call、approval、finished/cancelled/doom loop 等现有事件。Rust 序列化合同测试锁定关键 JSON，TypeScript union 和 reducer 负责穷尽消费。必须保持三种事件分离：

- Provider `ModelEvent`：厂商无关的模型语义流。
- Application Live Event：当前 UI 实时渲染，可丢失但终态可重新查询。
- Recorded Event：恢复、审计和 Projection 依赖的事实。

Tauri 只序列化和发送 Application Live Event，不直接发送 Provider DTO 或 Recorded Event。

## 9. 启动与退出生命周期

### 9.1 第一阶段

保留当前 fail-fast 启动：数据库、Schema 或加密配置无效时 Desktop 启动失败。该行为适合当前开发期，但必须返回可诊断的启动错误，不能静默建表。

### 9.2 后续产品化

当 Desktop 需要在数据库暂时不可用时仍展示设置/修复页面，再引入：

```text
Initializing -> Ready
             -> InitializationFailed
             -> ShuttingDown
```

不要在边界收口阶段同时引入复杂启动状态机。

### 9.3 Shutdown

`OpenWorkApplication::shutdown` 后续至少负责：

- 取消所有活跃 Turn。
- 结束或标记等待中的审批。
- 停止受管理后台任务。
- 阻止新的 Command 进入。
- 关闭 Persistence 资源或等待连接池自然退出。

## 10. Cargo 依赖收口

Phase A 之前 Desktop 直接依赖 `openwork-persistence`、`openwork-providers` 和未使用的 `openwork-workspace`。当前 `src-tauri` 已收口为：

```text
openwork-app
tauri
tauri-plugin-opener
dotenvy                  # 仅开发环境加载
serde                    # CommandError 序列化
serde_json               # 仅 dev-dependencies，供 IPC JSON 合同测试
```

原先未直接使用的 `async-trait`、`tokio-util` 和 `openwork-workspace` 已删除；`serde_json` 不进入生产依赖，仅用于 Desktop IPC JSON 合同测试。Desktop 对 Persistence、Providers 和 Protocol 的直接依赖也已移除。依赖收口由源码结构测试锁定，防止 Desktop 再次直接引入 Persistence/Providers/Execution。

## 11. 测试策略

### 11.1 App 合同测试

- Provider CRUD/Test 只通过 Application Service 完成。
- Session Query/Command 不暴露 `SessionStore`。
- Approval/Cancel 只路由到匹配 Turn。
- Application Error 映射不泄漏敏感配置。
- `[Phase C]` shutdown 会取消全部活跃 Turn。

### 11.2 Desktop Adapter 测试

- Tauri Command 只依赖 `OpenWorkApplication`。
- IPC DTO 与 TypeScript 类型字段保持一致。
- Application Error 稳定映射为 `CommandError`。
- Event 枚举序列化为前端期望的 tagged payload。
- 关键错误 JSON、Live Event JSON 有 Rust 合同测试。
- 前端错误解析和 stream accumulator 有 Vitest 行为测试。

### 11.3 源码结构测试

检查 `apps/desktop/src-tauri/Cargo.toml` 和 `src/commands`：

- 不依赖 `openwork-persistence`、`openwork-providers`、`openwork-execution` 或 `openwork-workspace`。
- 不出现 `SessionStore`、`ProviderFactory`、`ProviderRepositoryState`。
- 不新增读取 React/TypeScript 源文件的 Rust 测试。

“前端不展示 custom provider”等 UI 行为应由 Vitest/React 测试负责；Provider Preset 的 Rust 合同测试跟随 App Provider Catalog。

## 12. 分阶段实施

### Phase A：边界收口，保持前端协议（已完成）

1. [x] 新增 `OpenWorkApplication` 和 `ApplicationConfig`。
2. [x] 把现有 Provider CRUD/Test 迁入 App Service。
3. [x] 把现有 Session/Message Query/Command 迁入 App Service。
4. [x] 把 `RequestCancelRegistry` 与 `ChatRuntime` 收入 Turn Application Service。
5. [x] Tauri 只注册一个 Application State。
6. [x] 保持 Tauri Command 名、参数和返回 JSON 不变。
7. [x] 删除 Desktop 对 Persistence、Providers、Workspace 的直接依赖。

退出条件：现有 UI 无需修改即可运行；Desktop 源码不再引用 Repository、Store 或 Factory。

### Phase B：稳定 Transport 合同（已完成）

1. [x] 增加 `ApplicationError`、`CommandError` 与稳定错误码。
2. [x] 将字符串 Live Event 迁为 tagged enum。
3. [x] TypeScript 改为 discriminated union。
4. [x] 增加前端错误解析和流归并 Vitest 测试；Rust 源码结构测试只锁定依赖方向。

退出条件：前端不解析错误文案；Rust/TypeScript 都能穷尽匹配事件种类。

### Phase C：生命周期与发布安全

1. 增加 Application shutdown/cancel-all。
2. 根据产品需求决定是否增加初始化失败页面。
3. 配置 release CSP。
4. 收紧 Opener Capability。
5. 清理 Tauri Cargo 和应用元数据中的脚手架内容。

退出条件：退出时无悬挂 Turn；发布配置不再使用默认脚手架值。

## 13. 验收命令

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
pnpm --dir apps/desktop test
CI=true pnpm --dir apps/desktop build
```

当前已有源码结构测试验证 Desktop 只依赖 App 边界，不以人工检查 Cargo.toml 作为长期门禁。

## 14. 替代方案

### 14.1 保持当前 Tauri 直接组装

- 优点：代码少，短期无需迁移。
- 缺点：Desktop 持续知道 Persistence、Provider 和 Session 具体实现；第二个宿主会复制组装逻辑。
- 结论：拒绝，与唯一 Composition Root 约束冲突。

### 14.2 把业务逻辑搬到 React

- 优点：减少 Rust Command 代码。
- 缺点：API Key、数据库、文件权限和本地执行边界进入 WebView；核心用例无法被非 UI 宿主复用。
- 结论：拒绝。

### 14.3 立即增加本地 HTTP/WebSocket Server

- 优点：天然形成网络 API，可支持多个客户端。
- 缺点：增加监听端口、认证、版本、跨域和部署复杂度；当前只有一个桌面宿主。
- 结论：暂不采用；第二个真实宿主出现后重新评估。

### 14.4 建立通用 Command Bus

- 优点：形式统一，容易增加动态路由。
- 缺点：当前用例数量有限，会引入字符串路由、类型擦除或无收益的抽象。
- 结论：拒绝；V1 使用显式 Service 方法。

## 15. 后果与风险

### 正面后果

- Tauri 成为可替换的 Desktop Adapter。
- Application 用例可以被集成测试、CLI 或未来宿主复用。
- Persistence、Provider 和 Core 的变化不再要求修改 IPC Controller。
- 错误、事件和生命周期有单一所有者。
- Cargo 依赖方向与蓝图一致。

### 代价

- `openwork-app` 会新增 Provider/Session/Turn Service 和 DTO 映射代码。
- Event/Error 类型迁移需要同步 Rust 与 TypeScript。

### 风险与缓解

| 风险 | 缓解 |
| --- | --- |
| 重构同时改变前端协议导致回归 | Phase A 保持 Command 名和 JSON 不变 |
| App 变成新的“万能 crate” | 按 Provider/Session/Turn 用例拆模块，不把 SQL/HTTP Handler 搬入 App |
| 为未来宿主过度设计 | 不引入网络 Server、通用 Bus 或 Plugin API |
| 隐式泄漏底层实现 | 删除 Store/Repository getter，并增加依赖方向测试 |
| 启动生命周期范围膨胀 | Phase A 保持 fail-fast，初始化状态机延后 |

## 16. 决策记录

1. 接受 Tauri 作为当前 Desktop Host 技术，不将其视为业务领域模块。
2. 接受 `openwork-app` 作为唯一 Composition Root。
3. 接受第一阶段保留现有 `session_*` IPC 兼容名称。
4. 接受 Provider Preset 作为 App Query，而不是 Provider Adapter 合同。
5. 拒绝 Desktop 直接持有 Repository、Store 和 Factory。
6. 拒绝在第二个宿主出现前引入本地网络 Server 或通用 Command Bus。
7. Typed Error 与 Typed Live Event 已在 Phase B 完成；Release Security 留在 Phase C。
