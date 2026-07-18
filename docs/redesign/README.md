# OpenWork Runtime 重构设计

> 状态：V1 主运行链与 crate 收敛已于 `refactor/runtime-v1` 落地；Provider 凭证和模型已回填到 V2 表，旧表已归档为 `legacy_*` 仅供数据库回退。
>
> 源码基线：`grok-build` 与 OpenWork 当前工作区，最后核对于 2026-07-18。
>
> 本目录是本轮重构的唯一目标文档；其他 `docs/` 与 `plans/` 只作为历史资料。

## 1. 这次重构解决什么

重构前的 OpenWork 已经有模型流、工具执行、审批、Journal、Trace 和部分恢复代码，但同一条运行链被拆散在：

- `openwork-app::chat`：装配 Provider、Persistence、Trace、Supervisor 和 Core Agent；
- `openwork-core::agent`：Model → Tool → Model 循环，同时写 Step/ToolRun/Approval 事件和 Trace；
- `openwork-app::turn_supervisor`：活动 Turn 与审批命令；
- `openwork-persistence`：Session、Journal、生命周期回放；
- `openwork-observability` 与 `trace_service`：Trace 采集和查询。

因此，“工具执行完以后谁决定下一步”没有一个从目录和类型上都明确的答案。Trace 与恢复又建立在 `Turn → Step → ToolRun → Approval` 这套额外领域树上，使主循环更难读、更难改。

本次重构把问题归结为一件事：

> 一个常驻的 Session Runtime 必须完整拥有 Turn、Model Call、Tool Call、Permission 和下一轮 Model Call 的控制流。

## 2. 从 grok-build 借什么

直接参考的源码边界：

- `xai-grok-shell/src/session/acp_session.rs`：`SessionActor` 持有运行时依赖；
- `xai-grok-shell/src/session/acp_session_impl/run_loop.rs`：Session 命令循环；
- `xai-grok-shell/src/session/acp_session_impl/turn.rs`：源码中的 Prompt 处理和 Model → Tool → Model 循环；OpenWork 将这次完整运行命名为 Turn；
- `xai-grok-shell/src/session/acp_session_impl/tool_calls.rs`：Tool Call 准备、权限和执行；
- `xai-grok-agent/src/agent.rs`：构建完成的 Agent 定义，不是主循环；
- `xai-chat-state`：Conversation 的单写者；
- `xai-grok-tools` 与 `xai-grok-workspace`：参考工具表面、工作目录和权限边界；OpenWork V1 将这些能力收敛进 `openwork-tools`，不复制独立 Workspace 子系统；
- `xai-grok-shell/src/session/persistence.rs` 与 `chat_persistence.rs`：运行时决定写入顺序。

借用的是所有权关系，不是 crate 数量、文件规模或完整功能集。

本轮不复制 Grok 的：

- MCP；
- Compaction；
- Memory；
- Subagent；
- Plugin/Hook；
- Upload/Remote Sync；
- Rewind/Fork；
- 跨进程恢复未完成 Turn。

## 3. 核心决策

1. `openwork-core` 对应 `xai-grok-shell` 的产品运行时角色，而不是一个狭义状态机库。
2. `openwork-app` 的执行编排、Provider 凭证装配和 Host 转发全部合并进 `openwork-core`；`openwork-app` 删除，不新增 `openwork-shell`。
3. `SessionActor` 是每个活动 Session 的唯一运行时 Owner。
4. `openwork-agent` 只拥有 Agent Definition、System Prompt、Tool Set 和静态策略。
5. `openwork-chat-state` 是模型 Conversation 的唯一写入者。
6. Model Provider 统一进入 `openwork-models`；工具目录、文件/进程执行、工作目录上下文和路径权限统一进入 `openwork-tools`；V1 不设 `openwork-workspace` crate。
7. PostgreSQL Storage 与 Trace 作为 `openwork-core` 内部模块，不再各自形成产品级编排层。
8. 删除 `Step` 领域对象；循环次数只用局部 `model_call_index`。
9. 一次用户输入对应一个 `Turn`；`TurnId` 是运行、消息和 Trace 的稳定关联键。
10. `Prompt` 只表示 System/User Prompt 等模型指令内容；目标领域中不存在 `PromptId`、Prompt 状态或 `prompts` 表。
11. 第一阶段不恢复未完成 Turn。启动时只把遗留 `running` 状态标记为 `interrupted`。
12. Trace 是 best-effort 诊断数据，不能决定业务状态，也不能触发工具重放。
13. 目标数据库收敛为 7 张表：Provider Credential、Model、Session、Turn、Message、Trace 和 Migration；密文与公开模型配置分离。
14. Desktop 是 Core 的协议客户端：Command 提交意图/读取快照，Event 传递 Live Update，React 不复制 Runtime 状态机。

## 4. 目标主链路

```text
Desktop
  -> OpenWorkCore
      -> SessionHandle::start_turn(...)
          -> SessionCommand::StartTurn
              -> SessionActor
                  -> append User Message
                  -> loop
                      -> Model Call
                      -> append Assistant Message
                      -> if no Tool Call: complete
                      -> authorize + execute Tool Call(s)
                      -> append Tool Result Message(s)
                      -> continue Model Call
```

工具调用以后“下一步做什么”不由 Trace、数据库回放或 Desktop 决定，而由 `SessionActor` 中唯一的 Agent Loop 决定。

## 5. 目标仓库结构

```text
OpenWork/
├── apps/
│   └── desktop/                 # Tauri/React Host，只做适配与展示
└── crates/
    ├── openwork-core/           # Session Runtime、Storage、Trace、Host Facade
    ├── openwork-agent/          # Agent 定义、System Prompt 构建、静态策略
    ├── openwork-chat-state/     # Conversation Actor 与请求快照
    ├── openwork-models/         # 模型类型、Provider、Transport
    └── openwork-tools/          # Tool Catalog、文件/进程执行、路径权限、结果
```

这些 crate 已被吸收或删除：

```text
openwork-app            -> Provider/Runtime 入口进入 openwork-core；Host 错误 DTO 进入 Tauri；crate 删除
openwork-protocol       -> 类型随 Owner 移动
openwork-capabilities   -> openwork-tools
openwork-execution      -> openwork-tools
openwork-workspace      -> 删除；Git/Diff/Snapshot/Revert 不进入 V1
openwork-providers      -> openwork-models
openwork-persistence    -> openwork-core::storage
openwork-observability  -> openwork-core::trace
```

## 6. 文档顺序

1. [01-project-structure.md](01-project-structure.md)：目标 crate、模块、依赖方向和 current-to-target 映射。
2. [02-event-update-model.md](02-event-update-model.md)：SessionActor、Agent Loop、消息、Live Update、崩溃语义。
3. [03-database-schema.md](03-database-schema.md)：不做中途恢复时的 7 表 PostgreSQL Schema。
4. [04-trace-design.md](04-trace-design.md)：Turn 下的 Model Call/Tool Call 诊断 Trace。
5. [05-refactor-roadmap.md](05-refactor-roadmap.md)：分阶段迁移顺序、验收门槛和旧代码删除条件。
6. [06-frontend-architecture.md](06-frontend-architecture.md)：React/Tauri 边界、Host Contract、per-session Runtime Store 和页面拆分。

## 7. V1 明确不做什么

V1 不建立：

```text
event journal
step projection
tool_run projection
approval projection
runtime checkpoint
session_updates table
projection checkpoint
trace-driven recovery
git status/diff
worktree snapshot/revert
workspace trust subsystem
```

进程退出时：

- 已提交的 Message、Turn 汇总和 Trace 保留；
- 正在流式生成但未形成完整 Message 的内容可以丢失；
- 已开始但未记录结果的工具效果视为 `outcome_unknown`，绝不自动重试；
- 未完成 Turn 标记为 `interrupted`；
- 用户继续工作时创建新的 Turn。

以后若确实需要恢复，必须单独设计幂等键、副作用确认、Checkpoint 和恢复决策，不能把 Trace 或 Live Update 直接升级成恢复事实源。

## 8. 完成标准

重构完成后应满足：

- 从 `core/session/run_loop.rs` 能完整读懂一次 Turn 如何结束；
- 工具结果写回 Conversation 后必然进入下一次 Model Call，除非到达明确终态；
- 每种状态只有一个 Owner，不通过多个 service 交叉修改；
- Desktop 不直接组装 Provider、Recorder、Trace 或 Tool Executor；
- Desktop 只建立一个 Core Event Bridge，每个 Session 拥有独立 Runtime View；
- 已持久化 Message 与流式 Draft 分离，TypeScript DTO 由 Rust Host Contract 生成；
- `StepId`、`ToolRunId`、`ApprovalId` 和 `recorded_events` 不再出现在目标运行链；
- 目标依赖图中不存在 `openwork-workspace`；文件/进程工具始终受 `working_directory` 和 Permission Profile 约束；
- Trace 关闭或写入失败时，Agent Loop 行为完全不变；
- 进程重启不会重复执行未确认的工具副作用；
- `cargo test` 能覆盖无工具、单工具、多工具、工具失败、权限拒绝、取消、doom loop 和 Trace 降级。
