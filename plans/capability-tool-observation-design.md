# Capability、Tool 与 Observation V1 设计

> 文档地位更新（2026-07-16）：本文保留为 Capability/Execution 当前合同和演进背景，不再定义本轮目标 crate 边界。新的 Tools 与 Workspace 所有权以 [`docs/redesign/01-project-structure.md`](../docs/redesign/01-project-structure.md) 为准。
>
> 历史快照状态：V1 structure 与当时合同已经实现；Execution 风险出口与 Core/App 审批迁移已完成，Sandbox、Artifact 与 MCP Schema 兼容当时仍未完成。
> Last reviewed: 2026-07-15.
> Parent blueprint: [OpenWork Core 架构蓝图](./openwork-core-architecture-blueprint.md).

## 1. 要解决的问题

重构前，`openwork-tools` 把四类职责放在同一个 crate：

1. 向模型暴露的 Tool 名称、描述和 JSON Schema。
2. `ToolRegistry` 的发现与查找。
3. 文件、搜索和 Shell Handler 的真实 IO。
4. `ToolContext` 中的工作目录、权限和取消控制。

`openwork-agent` 因此曾直接依赖 `ToolRegistry`，既负责取得模型 Tool Schema，又直接调用具体 Handler。这个结构能够运行，但会产生三个确定的问题：

- 接入 MCP 后，Agent 必须知道本地 Registry 和远程 Tool 的不同实现。
- 参数校验、权限、沙箱、输出截断和错误归一化无法形成统一 Execution 边界。
- Tool 输出直接复用模型 `ContentBlock`，执行结果与模型消息表示没有独立演进空间。

重构前的测试只证明内置 Handler 和 Agent 审批流程可运行，没有锁定 Capability 发现、参数校验、Observation 归一化和依赖方向。当前已经增加 Capability 合同、Catalog、ExecutionService、Handler 名称对齐和源码结构测试。

## 2. 本次范围

当前 V1 已完成一个保持现有聊天路径可运行的纵向切片：

- 在 `openwork-protocol/capability` 落地当前最小 V1 类型和 Port；权限与审批专题允许在保持版本边界的前提下继续修订 Execution 合同。
- 新建 `openwork-capabilities`，只持有内置 Capability 声明与 Catalog。
- 新建 `openwork-execution`，持有 schema 校验、内置 Action Handler、权限上下文和 Invocation。
- `openwork-core` 只依赖 `CapabilityResolverPort` 与 `ExecutionPort`，不认识具体 Catalog/Handler。
- `openwork-app` 作为 Composition Root，组装 Catalog、Invoker、ExecutionService 和 Core Turn。
- 删除 `openwork-tools` crate，避免旧入口和新入口长期并存。

本次不做：

- 不实现 MCP、Skill 或 Plugin。
- 不实现 Tool Search、按需加载或模型侧自动路由。
- 不实现 macOS Seatbelt/Sandbox Runtime。
- Capability/Execution 不直接写 Recorded Event；Core 通过 `TurnRecorderPort` 持久化 Step/ToolRun/Approval 生命周期，Persistence Adapter 负责写入 `recorded_events`。
- 不实现 ArtifactStore；V1 继续在 Execution 内截断文本输出。
- 不声称支持完整 JSON Schema；只实现并测试当前内置 Tool 使用的受控子集。

## 3. 所有权与依赖方向

```text
openwork-core
  -> openwork-protocol::{CapabilityResolverPort, ExecutionPort}

openwork-app (Composition Root)
  -> openwork-core
  -> openwork-capabilities
  -> openwork-execution
  -> openwork-protocol

openwork-capabilities
  -> openwork-protocol

openwork-execution
  -> openwork-protocol
  -> openwork-execution/policy (执行权限配置与路径判定)
```

硬约束：

- `openwork-core` 不依赖 `openwork-capabilities` 或 `openwork-execution` 的具体类型。
- `openwork-execution` 不依赖 `openwork-capabilities`；它只接收注入的 `CapabilityResolverPort`。
- `openwork-capabilities` 不依赖 `openwork-execution`。
- Handler 不向模型生成 Tool Schema。
- Catalog 不执行文件、进程或网络 IO。
- Desktop 不再直接依赖工具实现 crate。

## 4. Protocol V1

### 4.1 CapabilitySpec

```rust
pub struct CapabilitySpec {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub risk_hint: CapabilityRiskHint,
}

pub enum CapabilityRiskHint {
    ReadOnly,
    WorkspaceMutation,
    ProcessExecution,
}
```

`risk_hint` 只是声明侧提示，不是最终权限结论。Execution 必须结合实际参数和运行位置判断；后续 MCP/Plugin 来源也不能靠声明自行获得权限。

当前 `ExecutionService::authorize` 会把 `risk_hint` 交给 Execution Policy，用于生成粗粒度审批原因。它不会自行降低权限或绕过 `ApprovalPolicy`，也不能替代后续针对实际参数、路径和运行位置的最终风险计算。

`CapabilitySpec` 可以转换为模型 `ToolDefinition`，但 Model 层不拥有 Catalog。

### 4.2 ActionRequest

```rust
pub struct ActionRequest {
    pub name: String,
    pub input: serde_json::Value,
}
```

V1 不把 provider tool-call id 当作 ToolRunId。真正的 `ToolRunId` 由 Durable Core 为每个工具调用创建；provider ID 只用于协议关联和向模型回填结果。

### 4.3 Observation

```rust
pub struct Observation {
    pub status: ObservationStatus,
    pub content: Vec<ObservationContent>,
    pub error: Option<ObservationError>,
}
```

终态：

```text
succeeded | failed | denied | cancelled | outcome_unknown
```

错误至少区分：

```text
capability_not_found
invalid_arguments
handler_not_found
approval_denied
permission_denied
cancelled
timeout
execution_failed
outcome_unknown
```

`ObservationContent` V1 只支持 Text。Artifact、结构化数据和二进制引用等到 Artifact 合同冻结后再加版本，不复用 Provider 原始 DTO。

### 4.4 Port

```rust
#[async_trait]
pub trait CapabilityResolverPort: Send + Sync {
    async fn list(&self) -> Result<Vec<CapabilitySpec>, CapabilityResolveError>;
    async fn resolve(&self, name: &str)
        -> Result<Option<CapabilitySpec>, CapabilityResolveError>;
}

#[async_trait]
pub trait ActionInvoker: Send + Sync {
    async fn invoke(&self, request: ActionRequest)
        -> Result<Observation, ActionInvokeError>;
}

#[async_trait]
pub trait ExecutionPort: Send + Sync {
    async fn authorize(
        &self,
        request: &ActionRequest,
        policy: ApprovalPolicy,
    ) -> ExecutionPolicyDecision;

    async fn execute(&self, request: ActionRequest) -> Observation;
}
```

当前分开保留三个边界：Resolver 允许未来异步发现 MCP 能力；Invoker 是前置检查后的 Handler 入口；ExecutionPort 的 `authorize` 负责输出 `Allow / Deny / RequireApproval`，`execute` 负责校验、调用 Handler，并把所有可预期失败归一化为 Observation。

Execution 不等待 UI。Core 根据 `authorize` 的结果决定直接执行、拒绝，或者进入 Waiting 状态并等待 `ResolveApproval` 命令；审批完成后才调用 `execute`。当前参数级最终风险与真实 Sandbox 仍未完成。

## 5. 当前源码结构

```text
openwork-capabilities/src/
├── builtin.rs            # 内置 CapabilitySpec
├── catalog.rs            # 注册、去重、list/resolve
└── lib.rs

openwork-execution/src/
├── actions/
│   ├── filesystem/       # read/write/edit/list/grep/glob
│   ├── process/          # bash
│   ├── output.rs         # 共享输出截断
│   └── mod.rs
├── context.rs            # 执行环境
├── handler.rs            # 内部 ActionHandler
├── invoker.rs            # Handler 路由
├── policy/               # ApprovalPolicy 评估与文件访问边界
├── schema.rs             # 受控 Schema 子集校验
├── service.rs            # authorize，以及 resolve -> validate -> invoke
└── lib.rs
```

## 6. 内置能力

| Capability | 声明风险提示 | Handler 所有者 |
| --- | --- | --- |
| `read` | `ReadOnly` | Execution |
| `list` | `ReadOnly` | Execution |
| `grep` | `ReadOnly` | Execution |
| `glob` | `ReadOnly` | Execution |
| `write` | `WorkspaceMutation` | Execution |
| `edit` | `WorkspaceMutation` | Execution |
| `bash` | `ProcessExecution` | Execution |

Catalog 和 Handler 名称由合同测试对齐。发现声明但没有 Handler 时必须返回 `handler_not_found`，不能 panic 或静默跳过。

## 7. Execution 时序

```text
Agent 从 CapabilityResolverPort.list 获取模型 schema
  -> Provider 返回 tool call
  -> Core 构造 ActionRequest
  -> ExecutionPort.authorize(request, approval_policy)
  -> RequireApproval 时 Core 进入 Waiting，等待 ResolveApproval
  -> Allow 或审批通过后调用 ExecutionPort.execute
  -> ExecutionService.resolve(name)
  -> 校验 input_schema
  -> ActionInvoker.invoke
  -> Handler 做路径/取消/超时检查并执行 IO
  -> ExecutionService 归一化 Observation
  -> Agent 把 ObservationContent 转为模型 ToolResultBlock
```

V1 schema 校验支持当前内置 Tool 所需子集：对象根、`properties`、`required`、`enum` 和基础 JSON 类型。`CapabilityCatalog` 当前只校验名称与描述，不会在注册时证明 Schema 属于该子集；不支持项会在 Execution 调用时成为参数校验错误。MCP 接入前必须选择完整 Draft 实现或明确兼容策略，不能把 V1 校验器宣称为通用 JSON Schema 引擎。

## 8. 失败、安全与重试

- 参数不合法：Execution 不调用 Handler，返回 `failed/invalid_arguments`。
- 未知 Capability：返回 `failed/capability_not_found`。
- 声明存在但 Handler 缺失：返回 `failed/handler_not_found`。
- 路径越权：返回 `denied/permission_denied`。
- 取消：返回 `cancelled/cancelled`。
- 进程超时：返回 `failed/timeout`。
- 普通 IO/进程失败：返回 `failed/execution_failed`。
- 无法确认副作用是否完成：保留 `outcome_unknown`，V1 内置 Handler 暂不主动产生。

Execution 不做语义重试。是否重试 Action 由后续 Core Recovery 根据幂等性和副作用阶段决定。

当前 `PermissionProfile` 仍只是应用层路径检查，`bash` 仍不是 sandbox。网络限制环境变量也不构成隔离保证。

## 9. 测试与验收

合同测试：

- Capability/Action/Observation 可序列化，状态和错误码稳定。
- Port 满足 `Send + Sync`。
- Catalog 拒绝空名称和重复名称。
- 内置风险提示正确。
- 缺少 required 参数时 Invoker 不被调用。
- 合法参数只调用一次 Invoker。
- Catalog 与 BuiltinActionInvoker 名称完全一致。
- Handler 按 `actions/filesystem`、`actions/process` 组织，不再保留含义模糊的 `builtin/` 目录。
- Agent 只通过两个 Protocol Port 获取 schema 和执行 Action。
- 原有 read/write/edit/search/bash、审批、取消和 doom-loop 测试保持通过。

架构测试：

- Workspace 不再包含 `openwork-tools`。
- Agent manifest 不依赖 Capabilities/Execution 具体 crate。
- Capabilities 与 Execution 不互相依赖。
- Desktop 不依赖工具实现。

最终验证：

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
CI=true pnpm --dir apps/desktop build
```

## 10. 发布与回滚

这是开发阶段的原子源码迁移，不保留双写或兼容 facade。回滚单位是本次源码变更集：

1. RED 合同测试与设计。
2. Protocol/Capabilities/Execution 实现及 Agent/Runtime 接线。
3. 文档与结构清理。

如果 Agent Golden Path 无法保持，回滚到旧 `openwork-tools` 源码基线；不在两个架构之间增加长期 adapter。

## 11. Decision Log

### D1：采用 Catalog 与 Handler 分离

接受。它直接解决 Agent 对具体 Registry 的依赖，并为 MCP/Skill 留出异步发现边界。

### D2：不把 `openwork-tools` 仅改名为 `openwork-capabilities`

拒绝。改名会继续把声明、权限上下文和真实 IO 放在一起，与蓝图依赖方向冲突。

### D3：V1 不引入完整 JSON Schema 新依赖

接受受控子集。当前七个内置能力只需要对象、required 和基础类型；完整实现必须在 MCP 专题中根据 Draft 兼容和依赖成本重新决定。

### D4：Capability 目录重构不附带迁移审批状态机

当时接受该过渡状态，因为审批迁移会同时改变 Core Command/Event、暂停恢复和 UI 路由，不能作为 Tool 目录移动的附带修改。后续权限专题已经完成迁移：Core 现在持有 Waiting 状态，Execution 只返回策略决定且不等待 UI。

### D5：不保留 `openwork-tools` compatibility crate

接受。当前只有 Workspace 内部消费者，长期 facade 会掩盖未完成迁移并增加双入口。
