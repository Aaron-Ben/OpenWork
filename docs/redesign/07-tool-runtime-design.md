# OpenWork 工具运行时四层分层设计

> 状态：四层运行时已实施，当前代码未提交。
>
> 日期：2026-07-18。
>
> 范围：`crates/openwork-tools`、`crates/openwork-agent` 与 `crates/openwork-core/src/session` 的工具定义、选择、注入和调用链。
>
> 决策：借用 `grok-build` 的工具生命周期分层，但不复制其 crate 数量、动态 MCP 注册和通用资源袋。

## 1. 决策摘要

OpenWork 将工具运行时收敛为四层：

1. **工具契约层**：一个工具类型同时拥有稳定 ID、描述、风险、输入/输出类型和执行逻辑；
2. **工具集选择层**：Agent 只声明当前角色允许使用哪些已注册工具；
3. **会话级上下文层**：Session 创建时注入工作目录、文件系统、进程后端、环境和不可绕过的 Permission Profile；
4. **调用级上下文层**：每次 Tool Call 单独注入 call ID、取消令牌、deadline 和未来可选的进度通道。

四层在 Session 创建时收敛成不可变的 `FinalizedToolset`。模型看到的 Tool Definitions 与运行时可以 dispatch 的工具必须来自同一个 `FinalizedToolset`。

本次不新增：

- `openwork-tool-runtime` crate；
- `openwork-workspace` crate；
- 动态 MCP 注册；
- 可变 `ToolBridge`；
- 通用 `TypeId -> Any` Resources 容器；
- Tool Pack、行为版本和参数重命名系统。

`openwork-tools` 在当前规模下同时承担轻量 Tool Runtime 与内置工具实现；只有出现多个独立工具提供方或远端执行边界时，才重新评估 crate 拆分。

## 2. 当前实现与问题

当前调用链是：

```text
OpenWorkCore::from_storage_parts
  -> ToolCatalog::builtin()

OpenWorkCore::build_session_handle
  -> AgentBuilder::build(&ToolCatalog)
  -> BuiltinToolExecutor::new(ToolContext)
  -> SessionRuntimeConfig {
       tools: Arc<ToolCatalog>,
       tool_executor: Arc<dyn ToolExecutor>,
     }

TurnRunner
  -> ToolCatalog::validate
  -> ToolCatalog::authorize
  -> ToolExecutor::invoke
  -> handler.name() 字符串查找
```

其中存在四份需要人工保持一致的信息：

1. `builtins/definitions.rs` 中的名称、描述、JSON Schema 和风险；
2. `BuiltinToolExecutor::new` 中注册的 handler 列表；
3. 每个 handler 的 `name()`；
4. handler 内部对 `serde_json::Value` 字段的手工读取。

当前七个工具恰好一致，但结构上没有以下保证：

- Catalog 中的每个 definition 一定存在 handler；
- handler 接受的字段一定与模型看到的 Schema 相同；
- Agent 没有选择的工具一定不能执行；
- Session 级 `ToolContext` 中的取消令牌一定属于当前 Turn；
- 取消 Tool Call 后，已启动的子进程一定终止。

另外，`Agent` 只保存从 Catalog 复制出的模型定义，执行时 `TurnRunner` 又使用全局 Catalog 和独立 Executor。这意味着“模型可见集合”和“可执行集合”并不是同一个运行时对象。

## 3. 目标与非目标

### 3.1 目标

- 一个具体工具只注册一次；
- Rust 输入类型是反序列化与 JSON Schema 的唯一事实源；
- Agent Tool Set 形成真实 executable subset；
- Session Context 与 Call Context 的生命周期严格分开；
- 模型 definitions 与 dispatch entries 来自同一个 finalized snapshot；
- Core 仍然拥有 Tool Call、Permission 等待、Trace 和 Conversation 写入顺序；
- Tools 仍然拥有不可绕过的路径、进程和网络安全边界；
- Turn cancellation 能传播到具体 Tool Call，并终止其进程或异步操作；
- 新设计保持在现有五个 Rust crate 内完成。

### 3.2 非目标

- 不实现 MCP client/server；
- 不支持运行期注册和注销工具；
- 不实现远端 Workspace Server；
- 不实现 Git Snapshot、Revert、Workspace Trust 或 Checkpoint；
- 不实现 Tool ABI 跨进程版本兼容；
- 不把 Permission 用户交互移动进 `openwork-tools`；
- 不改变 Model Adapter、数据库 Schema 或 Trace Schema。

## 4. 总体结构

```text
内置 Tool 类型
  -> ToolRegistryBuilder                 # 注册所有已知工具
      + AgentDefinition::tool_names      # 选择当前 Agent 工具集
      + ToolSessionContext               # 注入 Session 依赖
  -> FinalizedToolset                    # definitions + dispatch，同一快照

每次模型返回 Tool Call
  -> Core 校验、判定 Allow/Ask/Deny
  -> ToolCallContext                     # 注入本次 call 的取消/期限
  -> FinalizedToolset::call
  -> Typed Tool::execute
  -> ToolResult
  -> Core 写回 Conversation 并继续 Model Call
```

所有权关系：

```text
openwork-agent
  owns: Agent Definition、Tool Set 选择、静态策略

openwork-tools
  owns: Tool 契约、注册、finalize、dispatch、安全边界、内置工具

openwork-core
  owns: Session 组合根、Tool Call 生命周期、Permission 等待、取消、Trace、持久化
```

## 5. 第一层：工具契约

### 5.1 Typed Tool

具体工具使用带关联类型的强类型契约：

```rust
#[async_trait]
pub trait Tool: Send + Sync + 'static {
    type Input: DeserializeOwned + JsonSchema + Send + 'static;
    type Output: ToolOutput + Send + 'static;

    fn id(&self) -> ToolId;
    fn description(&self) -> &'static str;
    fn risk(&self) -> ToolRisk;

    async fn execute(
        &self,
        session: &ToolSessionContext,
        call: ToolCallContext,
        input: Self::Input,
    ) -> Result<Self::Output, ToolError>;
}
```

要求：

- `ToolId` 是稳定内部身份，不从文件名或类型名隐式推导；
- Input 通过 `serde` 反序列化；
- Input Schema 通过 `schemars` 从同一个 Input 类型生成；
- Tool Definition 由 `id + description + schema + risk` 生成；
- handler 不再手工读取 `Value::get("...")`；
- Output 统一转换成模型可消费的 `ToolResult`。

例如 `read`：

```rust
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadInput {
    /// Absolute path, or a path relative to the working directory.
    pub path: String,
}

pub struct ReadTool;

#[async_trait]
impl Tool for ReadTool {
    type Input = ReadInput;
    type Output = TextToolOutput;

    fn id(&self) -> ToolId {
        ToolId::new_static("read")
    }

    fn description(&self) -> &'static str {
        "Read a UTF-8 text file from the filesystem."
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }

    async fn execute(
        &self,
        session: &ToolSessionContext,
        call: ToolCallContext,
        input: ReadInput,
    ) -> Result<TextToolOutput, ToolError> {
        // 真实实现省略
    }
}
```

示例只表达契约，不要求名称或返回类型逐字照抄。

### 5.2 Object-safe dispatch adapter

关联类型 `Tool` 不直接作为 `dyn Tool` 保存。Registry 内部通过 `DynTool`/`ToolAdapter<T>` 做一次类型擦除：

```rust
#[async_trait]
trait DynTool: Send + Sync {
    fn definition(&self) -> ToolDefinition;
    fn risk(&self) -> ToolRisk;

    async fn call(
        &self,
        session: Arc<ToolSessionContext>,
        call: ToolCallContext,
        input: serde_json::Value,
    ) -> ToolResult;
}

struct ToolAdapter<T: Tool> {
    inner: T,
}
```

`ToolAdapter<T>` 负责：

1. 把 JSON 反序列化为 `T::Input`；
2. 把 finalized Session Context 与本次 Call Context 分别传给 `T::execute`；
3. 把 `T::Output` 或 `ToolError` 统一转换为 `ToolResult`。

这使具体工具保持强类型，运行时仍可以通过 `HashMap<ToolId, Arc<dyn DynTool>>` 分发。

## 6. 第二层：工具集选择

### 6.1 Registry 与 Toolset 是不同概念

```text
Registry = 当前二进制知道哪些工具
Toolset  = 当前 Agent 实际允许哪些工具
```

`ToolRegistryBuilder` 注册全部内置工具：

```rust
let registry = ToolRegistryBuilder::new()
    .register(ReadTool)
    .register(WriteTool)
    .register(EditTool)
    .register(GrepTool)
    .register(GlobTool)
    .register(ListTool)
    .register(BashTool);
```

Agent Definition 继续使用稳定 ID 选择：

```rust
pub struct ToolsetConfig {
    pub tools: Vec<ToolId>,
}
```

`AgentDefinition::tool_names` 可以先保留现有序列化形式，在进入工具运行时时转换为 `ToolsetConfig`。

### 6.2 Finalize 校验

Finalize 必须拒绝：

- 空白 Tool ID；
- 重复注册；
- Toolset 引用未知工具；
- Toolset 内重复选择；
- 模型侧名称冲突。

Finalize 只把选中的工具写入最终 dispatch map。没有进入 Toolset 的工具既不发给模型，也不能被 `FinalizedToolset::call` 执行。

因此，即使 Provider 返回了一个未提供给模型的工具名，运行时也必须返回 `ToolNotFound`，不能回退到全局 Registry。

## 7. 第三层：会话级上下文

`ToolSessionContext` 的生命周期与活动 Session 一致：

```rust
pub struct ToolSessionContext {
    pub working_directory: PathBuf,
    pub permissions: PermissionProfile,
    pub environment: Arc<HashMap<String, String>>,
    pub filesystem: Arc<dyn AsyncFileSystem>,
    pub process_backend: Arc<dyn ProcessBackend>,
}
```

这层只保存同一个 Session 中可复用且语义稳定的依赖：

- 工作目录；
- 文件读写后端；
- 进程启动和终止后端；
- Session 环境变量；
- 路径、进程和网络的强制安全策略。

这层不保存：

- Turn Cancellation Token；
- Tool Call ID；
- 当前 Permission Request；
- SessionActor；
- Chat State；
- ModelPort；
- 数据库连接；
- Trace Recorder。

Tools 可以依赖 `ToolSessionContext`，但不能反向依赖 `openwork-core`。

### 7.1 为什么注入 Filesystem 和 ProcessBackend

当前内置工具直接调用 `tokio::fs` 和 `tokio::process::Command`。抽象成两个受控后端后：

- 单元测试不需要访问真实工作目录；
- 路径检查可以在唯一入口强制执行；
- 进程表、kill、timeout 和取消语义集中管理；
- 将来接入 sandbox 时不需要修改每个工具的业务参数；
- 本地实现仍然可以简单包装 Tokio，不要求建立独立 workspace 服务。

这两个后端属于 `openwork-tools`，不构成新增 `openwork-workspace` crate 的理由。

## 8. 第四层：调用级上下文

`ToolCallContext` 每次调用创建一次：

```rust
#[derive(Clone)]
pub struct ToolCallContext {
    pub call_id: ToolCallId,
    pub cancel: CancellationToken,
    pub deadline: Option<Instant>,
}
```

V1 最少只需要 `call_id + cancel`。`deadline` 可以在统一 timeout 时引入。

未来可以增加：

- progress sender；
- per-call cwd override；
- 只读的 trace correlation string；
- 调用来源，例如 primary agent/subagent。

不直接放入 `SessionId`、`TurnId` 等 Core 类型，避免 `openwork-tools -> openwork-core` 反向依赖。Session/Turn 关联由 Core 的 Trace 和事件层维护；工具只需要知道当前调用身份和取消信号。

### 8.1 取消不变量

- Session 创建时不创建永不更新的 Tool Cancellation Token；
- 每个 Tool Call 从当前 Turn token 派生自己的 child token；
- 用户取消 Turn 时，所有活动 Tool Call token 同时取消；
- timeout 与用户取消最终进入同一个进程终止路径；
- 不允许把持有 Child 的 future 放入脱离管理的 `tokio::spawn` 后丢弃 JoinHandle；
- ProcessBackend 不得遗留失去 owner 的子进程；kill 确认失败时必须继续持有进程句柄并明确返回 `outcome_unknown`；
- `kill_on_drop` 只能作为最后防线，不能代替显式取消协议。

## 9. FinalizedToolset

`FinalizedToolset` 是四层的收敛点：

```rust
pub struct FinalizedToolset {
    definitions: Vec<ModelToolDefinition>,
    tools: HashMap<ToolId, Arc<dyn DynTool>>,
    session: Arc<ToolSessionContext>,
}
```

对外提供：

```rust
impl FinalizedToolset {
    pub fn definitions(&self) -> &[ModelToolDefinition];
    pub fn resolve(&self, id: &ToolId) -> Option<&ToolDefinition>;

    pub fn authorize(
        &self,
        invocation: &ToolInvocation,
        mode: PermissionMode,
    ) -> PolicyDecision;

    pub async fn call(
        &self,
        context: ToolCallContext,
        invocation: ToolInvocation,
    ) -> ToolResult;
}
```

实现可以进一步提供私有 `PreparedToolCall`，确保输入只解析一次，并把校验结果稳定地传过 Permission 等待阶段；这不是首轮切换的强制条件。

### 9.1 不变量

1. `definitions` 与 `tools` 使用相同的 selected entries 构造；
2. finalize 后不可增加、删除或替换内置工具；
3. `call` 只能在 finalized map 中查找；
4. schema、风险和执行 adapter 来自同一个注册项；
5. Session Context 在 finalize 时绑定；
6. Call Context 由 Core 在每次调用时提供；
7. Core 只持有一个 `Arc<FinalizedToolset>`，不再同时持有 Catalog 和 Executor。

由于 V1 没有动态 MCP，当前不需要 `ToolBridge`。如果未来出现动态工具，可以在 immutable built-in toolset 外增加独立动态层，而不是让当前对象提前可变。

## 10. Permission 边界

Permission 分成两类，不能混为一层：

### 10.1 用户决策

Core 负责：

```text
Allow -> 立即执行
Ask   -> 发出 PermissionRequest，等待用户
Deny  -> 返回 denied，不调用工具
```

原因：Permission 等待会修改 Session Runtime 状态、发 Live Update，并接收 Desktop 命令，它属于 `SessionActor` 的控制流。

### 10.2 执行期强制策略

Tools 负责：

- 目标路径是否位于允许范围；
- 是否允许写入保护目录；
- 是否允许网络访问；
- 进程参数是否满足限制；
- symlink/canonical path 是否越界。

用户选择 `Allow` 不能绕过这些安全边界。Tool contract 的 `risk()` 用于 Core 的用户决策，`ToolSessionContext.permissions` 用于执行期强制检查。

## 11. Core 组合与调用顺序

### 11.1 Session 创建

```rust
let agent = AgentBuilder::new(definition).build()?;

let toolset = builtin_registry()
    .finalize(
        agent.toolset_config(),
        ToolSessionContext {
            working_directory,
            permissions,
            environment,
            filesystem,
            process_backend,
        },
    )?;

SessionRuntimeConfig {
    agent,
    tools: Arc::new(toolset),
    // model/chat/storage/trace ...
}
```

`openwork-agent` 只声明工具集，不自行构造文件系统、进程或取消令牌。

### 11.2 Tool Call

```text
Provider ToolCallBlock
  -> 从 FinalizedToolset resolve/validate
  -> doom-loop 检查
  -> risk + PermissionMode -> Allow/Ask/Deny
  -> Ask 时由 SessionActor 等待用户
  -> 从当前 Turn cancel 派生 call cancel
  -> FinalizedToolset::call(ToolCallContext, invocation)
  -> 记录 Tool Trace
  -> append Tool Result Message
  -> 下一次 Model Call
```

未知工具或无效输入在请求用户授权之前失败。执行期路径策略仍在 Tool 内部或受控 backend 中再次强制检查。

## 12. 目标文件结构

保持单一 `openwork-tools` crate：

```text
crates/openwork-tools/src/
├── lib.rs
├── tool.rs                 # Tool/DynTool/Adapter、ID、Call Context、Result
├── registry.rs             # RegistryBuilder、ToolsetConfig、FinalizedToolset
├── policy/
│   ├── mod.rs
│   ├── evaluator.rs
│   ├── filesystem.rs
│   └── profile.rs
└── builtins/
    ├── mod.rs              # builtin_registry() 的唯一组合入口
    ├── filesystem/
    │   ├── read.rs
    │   ├── write.rs
    │   ├── edit.rs
    │   ├── grep.rs
    │   ├── glob.rs
    │   └── list.rs
    └── process/
        └── bash.rs
```

`context.rs`、`definition.rs`、`invocation.rs`、`result.rs` 是否最终合并进 `tool.rs`，以可读性为准；这不是架构不变量。真正需要删除的是两套注册链，而不是为了减少文件数强行合并所有类型。

### 12.1 Current-to-target

| 当前文件/类型 | 目标 | 处理 |
| --- | --- | --- |
| `builtins/definitions.rs` | 每个 `Tool` 实现 | 删除集中式手写 definitions/schema |
| `handler.rs::ToolHandler` | `Tool` + `ToolAdapter<T>` | 用 typed contract 替代字符串 handler |
| `executor.rs::BuiltinToolExecutor` | `FinalizedToolset::call` | 删除独立 handler 列表 |
| `catalog.rs::ToolCatalog` | `ToolRegistryBuilder` + `FinalizedToolset` | 区分已知工具和当前工具集 |
| `context.rs::ToolContext` | `ToolSessionContext` + `ToolCallContext` | 拆开 Session/Call 生命周期 |
| `Agent.tools: Vec<ModelToolDefinition>` | `Agent.toolset: ToolsetConfig` | Model Call 从 `FinalizedToolset::definitions()` 读取定义，不再复制独立事实源 |
| `SessionRuntimeConfig.tools + tool_executor` | `Arc<FinalizedToolset>` | 合并注入对象 |
| `TurnRunRequest.tools + tool_executor` | `Arc<FinalizedToolset>` | 合并调用入口 |

## 13. 迁移结果

本轮已完成 T1-T4 的生产调用链切换：新旧链没有同时保留，Core 已只持有 `Arc<FinalizedToolset>`。T0 中与结构和主调用顺序相关的 characterization tests 已补齐；更细粒度的每个文件工具 I/O 场景仍作为后续常规测试扩展，不阻塞本次架构收敛。

### T0：冻结当前行为

先补 characterization tests：

- 七个当前 Tool Definitions 快照；
- 七个 handler 的成功/失败输出；
- Unknown Tool；
- Invalid Arguments；
- Permission Allow/Ask/Deny；
- Bash timeout；
- Turn cancellation；
- Tool Result 后继续 Model Call。

### T1：引入新契约（已完成）

- 新增 `Tool`、`DynTool`、`ToolAdapter<T>`；
- 新增两个 Context；
- 新增 RegistryBuilder 和 FinalizedToolset；
- 切换完成后已删除旧 Catalog/Executor。

### T2：迁移七个内置工具（已完成）

- 每个工具增加 typed Input；
- 用 `schemars` 生成 schema；
- 先迁移 `read` 与 `bash` 验证文件/进程两类边界；
- 再迁移其余五个工具；
- 对比新旧 definitions 和结果语义。

### T3：切换 Agent 与 Core（已完成）

- Agent Definition 生成 `ToolsetConfig`；
- Session 创建时 finalize；
- `SessionRuntimeConfig` 和 `TurnRunRequest` 只传 `Arc<FinalizedToolset>`；
- 当前 Turn token 进入 `ToolCallContext`；
- Permission 和 Trace 顺序保持不变。

### T4：删除旧链（已完成）

- 删除 `builtin_definitions()`；
- 删除 `ToolHandler`；
- 删除 `BuiltinToolExecutor`；
- 删除独立 `ToolCatalog`；
- 删除旧 `ToolContext.cancel`；
- 更新测试 fake，使其注册 Fake Tool，而不是注入 Fake Executor。

### T5：验证与文档收口（已完成）

- 运行 Tools/Agent/Core 定向测试；
- 运行 workspace check；
- 搜索旧类型引用；
- 更新 `01-project-structure.md` 和 `05-refactor-roadmap.md` 的实施状态。

## 14. 验收标准

### 14.1 结构

- 一个工具只在 `builtin_registry()` 注册一次；
- 不存在独立 definitions 列表和 handlers 列表；
- Core 不再持有 `ToolCatalog + ToolExecutor`；
- Agent 不持有与 dispatch 分离的 definitions 副本；
- `openwork-tools` 不依赖 `openwork-core`；
- 不新增 `openwork-tool-runtime` 或 `openwork-workspace` crate。

### 14.2 行为

- 模型 Definitions 与 executable subset 完全一致；
- 未选择的工具返回 `ToolNotFound`；
- Typed Input 拒绝缺失字段和错误字段类型；
- Read-only Toolset 无法执行 write/edit/bash；
- Permission Ask 仍由 SessionActor 等待和恢复；
- Allow 不能绕过路径/进程强制策略；
- 取消 Turn 会终止正在运行的 Bash 子进程；
- timeout 后不存在遗留进程；
- Tool Result 后仍由同一个 TurnRunner 发起下一次 Model Call；
- Trace 失败不改变 Tool Result。

### 14.3 测试命令

```sh
cargo fmt --all -- --check
cargo test -p openwork-tools
cargo test -p openwork-agent
cargo test -p openwork-core
cargo check --workspace
```

结构搜索：

```sh
rg -n 'ToolCatalog|BuiltinToolExecutor|ToolHandler' crates/openwork-* --glob '*.rs'
rg -n 'CancellationToken::new\(\)' crates/openwork-core/src/core.rs crates/openwork-tools/src
```

实施完成后，第一条应为空或只存在于明确的迁移说明；第二条不应再显示 Session 创建时为 Tool Executor 固定创建取消令牌。

## 15. 备选方案与取舍

### 15.1 保持 Catalog + Executor 双对象

- 优点：改动最少；当前七个工具能够工作；
- 缺点：定义、schema、handler 和 executable subset 继续靠人工同步；
- 不采用原因：无法通过类型和 finalize 过程建立一致性不变量。

### 15.2 立即新增 `openwork-tool-runtime`

- 优点：协议与内置实现形成物理 crate 边界；未来可供 MCP/远端复用；
- 缺点：当前只有一个工具提供方，会提前产生公开 API、注册入口和依赖管理成本；
- 不采用原因：本轮目标可以在 `openwork-tools` 内完整实现，先验证边界，再决定是否抽取。

### 15.3 新增 `openwork-workspace`

- 优点：将来可以集中 Git、Sandbox、Checkpoint 与远端执行；
- 缺点：当前只有 cwd、路径权限和直接文件/进程执行，没有独立 Workspace 生命周期；
- 不采用原因：V1 不实现 Git Snapshot/Revert、Workspace Trust 或 Workspace Daemon。

### 15.4 完整复制 grok-build ToolBridge/Resources

- 优点：动态工具、资源扩展、行为版本和远端执行能力完整；
- 缺点：与当前七个静态工具和无 MCP 的范围不匹配；类型擦除资源袋会降低依赖可见性；
- 不采用原因：只借用生命周期边界和 finalize 思想，不复制规模复杂度。

## 16. 后果

### 16.1 正面

- 工具协议和实现不会再静默漂移；
- 只读 Agent 等角色可以形成真实安全边界；
- cancellation、timeout 和进程清理由清晰生命周期承载；
- 单元测试可以注入文件系统和进程后端；
- 增加第八个内置工具时只需定义类型并注册一次；
- 未来增加 MCP 时可以复用 Tool contract，而不推翻内置工具。

### 16.2 代价

- 需要增加 `schemars` 和 object-safe adapter；
- 七个工具都要从手工 `Value` 读取迁移为 typed input；
- Core 测试中的 Fake Executor 需要改为 Fake Tool/Toolset；
- ProcessBackend 必须明确进程 ownership 和 kill 协议；
- 首轮迁移会同时存在新旧两条内部实现，但生产调用链不能同时执行两套工具。

### 16.3 风险与缓解

| 风险 | 缓解 |
| --- | --- |
| schema 生成结果改变 Provider 请求 | 为七个 definitions 建立快照/等价性测试 |
| typed input 比旧解析更严格 | 先记录当前容错行为，对需要兼容的字段添加 serde default/alias |
| cancellation 切换造成进程泄漏 | 增加真实长进程取消集成测试，并检查进程终止 |
| Toolset 过滤漏掉默认工具 | finalize 时校验 Agent 配置，并覆盖默认/只读工具集测试 |
| 重构改变 Permission 顺序 | 固定 Unknown/Invalid/Ask/Allow/Deny 的事件顺序测试 |

## 17. 未来拆分触发条件

只有出现以下至少两个条件时，重新评估 `openwork-tool-runtime`：

- MCP 成为正式工具提供方；
- 插件或第三方 Tool Pack；
- 本地与远端工具共享同一协议；
- 多个 crate 只需要 Tool contract，不应依赖内置文件/进程实现；
- Tool ABI 需要版本管理；
- 独立工具服务器需要复用 Registry/Dispatch。

只有出现以下能力时，重新评估 `openwork-workspace`：

- Git/Worktree 生命周期；
- Snapshot/Diff/Revert；
- Workspace Trust；
- Sandbox/Checkpoint；
- 本地与远端 Workspace 路由；
- 多个消费者共享同一个 Workspace Owner。

crate 数量不是触发条件，独立所有权、多个消费者和稳定协议边界才是。
