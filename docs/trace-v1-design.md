# OpenWork Trace V1 设计

> 状态：已实现
>
> 范围：本地桌面端的单 Session、单 Turn 执行诊断
>
> 非目标：LangSmith 式团队可观测平台、Eval 平台或分布式追踪平台

## 1. 为什么现在需要 Trace

OpenWork 已经具备可持久化、可重建并支持审批等待恢复的 Turn 生命周期。`recorded_events` 能回答“哪些业务事实已经发生”，但还不能完整回答：

- 当前 Turn 执行到了哪一步；
- 模型、审批和工具分别消耗了多少时间；
- 一次模型请求是否发生 Transport 重试；
- 首 Token 延迟和 Token 用量是多少；
- 某个失败属于 Provider、工具还是恢复流程；
- 应用重启后是否继续写入同一条执行链路。

Trace V1 用于补齐诊断信息，不替代 Event Journal，也不改变恢复语义。

## 2. 三类数据必须分开

| 数据 | 作用 | 可靠性 | 当前载体 |
|---|---|---|---|
| Recorded Event | 恢复、审计和状态重建依赖的事实 | 写入失败会阻止后续副作用 | `recorded_events` |
| Trace Span | 耗时、错误、重试和执行链路诊断 | Best effort，可删除、可重建一部分 | `trace_spans` |
| Live Event | 当前页面的流式文本和运行进度 | 允许页面刷新或退出后丢失 | Tauri `chat-stream-event` |

Trace 写入失败不能让 Agent Turn 失败。审批是否已批准、工具是否已执行等业务事实仍以 Recorded Event 为准。

## 3. V1 用户旅程

### 3.1 查看一轮执行摘要

作为用户，我希望在 AI 回答下看到本轮的步骤数、工具数、耗时、Token 和失败状态，从而判断这一轮是否正常。

### 3.2 定位失败节点

作为用户，我希望点击执行摘要后看到 Turn 的执行树，并能定位失败的模型请求、审批或工具调用。

### 3.3 区分审批等待与工具执行

作为用户，我希望分别看到审批等待时间和工具实际执行时间，避免把等待用户操作误判为工具性能问题。

### 3.4 查看模型重试

作为用户，我希望看到同一模型调用下的 Transport Attempt，以及每次失败的归一化错误和重试延迟。

### 3.5 查看恢复链路

作为用户，我希望应用重启并恢复待审批 Turn 后，Trace 仍属于原 Turn，并明确显示一次 Recovery。

## 4. Trace 层级

一个 Turn 对应一条 Trace，`trace_id` 固定使用 `turn_id`：

```text
Turn
├── Step
│   ├── Model Attempt
│   │   ├── Transport Attempt 1
│   │   └── Transport Attempt 2
│   ├── Tool Run
│   │   └── Approval
│   └── Tool Run
└── Recovery
```

V1 Span 类型：

| `span_kind` | 含义 | 稳定关联 ID |
|---|---|---|
| `turn` | 一轮用户请求的根节点 | `turn_id` |
| `step` | Agent Loop 的一步 | `step_id` |
| `model_attempt` | 一次逻辑模型调用 | `model_attempt_id` |
| `transport_attempt` | Provider Gateway 的一次真实请求尝试 | `model_attempt_id + attempt` |
| `tool_run` | 一次工具请求及执行 | `tool_run_id` |
| `approval` | 一次审批等待 | `approval_id` |
| `recovery` | 应用重启后的恢复动作 | 新生成 Span ID |

`verification` 暂不作为独立运行阶段；当前验证命令仍然表现为普通 Tool Run。等 Core 出现稳定 Verification 阶段后再新增该类型。

## 5. Span 状态

统一状态：

```text
running
waiting
succeeded
failed
cancelled
denied
outcome_unknown
```

Turn 的业务状态仍由 Turn 生命周期投影提供，例如 `waiting_approval`、`doom_loop` 和 `interrupted`。Trace 状态用于诊断展示，不参与恢复决策。

## 6. 持久化模型

V1 新增一张 `trace_spans` 表：

| 字段 | 类型 | 说明 |
|---|---|---|
| `span_id` | TEXT PK | Span 稳定 ID |
| `trace_id` | TEXT | 根 Trace ID，V1 等于 Turn ID |
| `parent_span_id` | TEXT NULL | 父 Span |
| `span_kind` | TEXT | Span 类型 |
| `span_name` | TEXT | 用户可读的稳定名称 |
| `status` | TEXT | 当前诊断状态 |
| `session_id` | TEXT | 所属 Session |
| `turn_id` | TEXT | 所属 Turn |
| `step_id` | TEXT NULL | 所属 Step |
| `tool_run_id` | TEXT NULL | 关联 Tool Run |
| `started_at` | TIMESTAMP | 无时区存储，按东八区解释和展示 |
| `ended_at` | TIMESTAMP NULL | 结束时间 |
| `attributes_json` | JSONB | 受限、脱敏的类型特定属性 |
| `error_type` | TEXT NULL | 归一化错误类型 |
| `error_code` | TEXT NULL | HTTP、Provider 或内部错误码 |
| `error_message` | TEXT NULL | 脱敏、截断后的错误信息 |
| `created_at` | TIMESTAMP | 创建时间 |
| `updated_at` | TIMESTAMP | 最后更新 |

`duration_ms` 不单独持久化，查询 DTO 根据 `started_at` 和 `ended_at` 计算，避免与时间字段产生不一致。

索引：

- `(session_id, started_at)`：加载 Session 下的 Turn 摘要；
- `(turn_id, started_at, span_id)`：加载一棵 Turn Trace；
- `(trace_id, parent_span_id)`：构建父子树；
- 失败 Span 的部分索引：后续出现全局失败查询需求时再添加。

Trace 使用 UPSERT：开始信号创建 `running` Span，结束信号以同一 `span_id` 更新终态。数据库中更早的 `started_at` 不能被恢复过程覆盖。

## 7. 各节点记录内容

### 7.1 Turn

- Provider ID；
- Model；
- 审批策略；
- 完成、失败、取消或 Doom Loop；
- 总耗时；
- 是否经过恢复。

### 7.2 Model Attempt

- Model；
- Finish reason；
- 输入、输出、缓存输入、Reasoning 和总 Token；
- 首个语义输出时间；
- Provider request ID；
- Transport Attempt 数量；
- 归一化 ModelError。

### 7.3 Transport Attempt

- Attempt 序号；
- 是否成功；
- HTTP 状态；
- Provider error code；
- Provider request ID；
- 是否继续重试；
- Retry delay。

### 7.4 Tool Run

- 工具名称；
- Provider tool call ID；
- 是否需要审批；
- 请求到终态的总时间；
- 实际执行开始时间；
- Observation 状态；
- 输出大小和是否截断由后续 Artifact 专题补充。

工具输入和输出不复制进 Trace。详情页通过 `tool_run_id` 使用现有生命周期投影中的参数和 Observation。

### 7.5 Approval

- 关联 Tool Run；
- 触发原因；
- Allow 或 Deny；
- 等待时间。

### 7.6 Recovery

- 恢复原因；
- 恢复前 Turn 状态；
- 继续的 Step；
- 关联 Approval。

## 8. 默认禁止记录的内容

以下数据不得进入 `trace_spans`：

- API Key、Authorization、Cookie；
- 数据库连接串和完整环境变量；
- 完整 System Prompt；
- 完整模型输入和输出；
- 完整文件内容；
- 原始 Provider Header 和 SSE；
- 未限制长度的命令输出。

`attributes_json` 只能放稳定白名单字段。错误信息写入前必须截断，并继续使用 Provider 层已经完成的错误归一化和脱敏结果。

## 9. 前端 V1

### 9.1 会话内摘要

AI 回答下显示：

```text
运行 3 步 · 模型 3 次 · 工具 2 次 · 18.6 秒 · 8,420 Tokens
```

失败时显示失败节点数量；发生恢复时显示“已恢复”。摘要点击后打开详情。

### 9.2 详情面板

右侧面板结构：

```text
Header：模型、总耗时、重试、错误
└── Turn -> Step -> Model/Transport/Tool/Approval/Recovery 时间线
```

详情面板加载轻量 Span 列表，并以内联方式显示归一化错误。工具参数和 Observation 从现有 Turn Snapshot 关联，不在 Trace API 中重复返回。

### 9.3 导航

- 点击 AI 回答下方摘要，打开对应 Turn；
- 点击工具活动行并定位对应 Tool Span 属于后续增强；
- 会话顶部 `...` 的 Session Trace 列表不属于首个实现切片，等单 Turn 详情稳定后添加。

## 10. 模块所有权

| 模块 | 责任 |
|---|---|
| `openwork-protocol` | Trace 类型、Repository/Recorder Port、Transport Attempt 信号 |
| `openwork-core` | 在 Model、Step、Tool、Approval 的语义点产生 Trace 信号 |
| `openwork-providers` | Gateway 产生真实 Transport Attempt 和 Retry 信号 |
| `openwork-observability` | 缓冲、关联、Span 状态归并和查询投影 |
| `openwork-persistence` | `trace_spans` 迁移与 PostgreSQL Repository |
| `openwork-app` | Trace Query Service、Turn 根 Span 与恢复编排 |
| `apps/desktop/src-tauri` | 薄 Tauri Query Command |
| `apps/desktop/src` | 摘要、详情面板、树和语义详情 |

Trace 记录器必须是 Best effort。Core 和 Providers 不依赖 PostgreSQL，也不能因为 Trace 写入失败而改变业务结果。

## 11. 本次实现切片

必须完成：

1. `trace_spans` migration 和 Repository；
2. Turn、Step、Model Attempt、Tool Run、Approval、Recovery Span；
3. Provider Gateway Transport Attempt/Retry 信号；
4. Session 下 Trace 摘要查询和单 Turn Trace 查询；
5. 会话内摘要和右侧详情面板；
6. Trace 在应用重启后仍可查询；
7. 三种界面语言；
8. Rust 单元/集成测试和前端组件测试。

明确不做：

- Dashboard、告警和成本；
- Trace 对比和分享；
- Dataset、Feedback、Annotation Queue 和 Eval；
- 分布式 Trace；
- OpenTelemetry 导出；
- 全局 Trace 搜索；
- 完整 Prompt/Response 捕获。

## 12. 验收标准

1. 普通对话完成后，AI 回答下出现对应 Turn 摘要；
2. 打开详情能看到真实父子关系，而不是按时间戳猜 Turn；
3. Model、Tool 和 Approval 耗时可区分；
4. Provider 重试时能看到多个 Transport Attempt；
5. Provider/Tool 失败能定位错误节点；
6. 待审批 Turn 重启恢复后仍使用原 `trace_id`，并新增 Recovery Span；
7. Trace 写入失败不导致 Turn 失败；
8. 迁移可重复执行，旧数据库通过显式 migration 升级；
9. Trace 中不存在 API Key、Authorization、完整 Prompt/Response；
10. Rust 相关测试、前端测试、TypeScript 检查和生产构建通过。
