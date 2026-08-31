# Collaboration 架构收口

本文记录 `raw.txt` 41 项决定完成后的代码审查结论，以及把当前实现收口为目标架构所需的修改。它是本轮代码修改的验收依据；发生冲突时，仍以 `raw.txt` 中较晚的明确决定为准。

## 目标

本轮不增加新产品功能，不接入 Codex，也不改变以下已经确定的产品边界：

- 单台 macOS、单个 Desktop 生命周期、单个 Collaboration Runtime；
- Desktop 监督一个 Server 子进程和一个 Computer 子进程；
- 当前生产只注册本机 OpenCode；
- PostgreSQL 保存持久事实，Redis/SSE 只负责短期协调和 invalidation；
- 每个 Agent 拥有独立 Engine 配置、Runtime、持久 home 和临时凭证；
- 不增加远程 Mac、后台常驻、Memory、Skills、reaction、Climate history 或 WebSocket。

需要修复的是：目标语义已经写进文档，但部分主控制流、模块接口和故障验收尚未真正闭合。

## 一、多 Engine 主控制流

### 当前问题

`EngineRegistry` 和 per-Agent `AgentEngineRuntime` 已存在，但 Server 只接受 `opencode`，Computer 只探测 OpenCode，并用 OpenCode 的 readiness 决定全部 Runner 是否运行。这样新增第二个真实 Adapter 时，仍必须修改 Server、inventory 和 daemon 主循环。

### 最终模块与接口

- `EngineId` 是 protocol 中的强类型领域值，Server 和 Computer 共用相同格式校验，但 Server 不维护 Engine allowlist。
- Server 持久化每个 Agent 的 `engine_id`，不依赖 Computer 的 Adapter 实现。
- `EngineRegistry` 是 Computer 内唯一 Adapter 注册表，并可枚举所有已注册 Adapter。
- Computer 对每个 Adapter 独立 probe，按 `engine_id` 上报 inventory/readiness。
- reconcile 按每个 Agent 自己的 `engine_id` 判断是否可以启动；一个 Engine 缺失不能停止其他 Engine 的 Runner。
- 当前生产构造仍只注册 OpenCode。加入 Codex 时只新增并注册真实 Adapter，再在 Desktop 中显示新选项；不增加 Codex 占位实现。

### 验收

- Server 能保存任意格式合法的 Engine ID，格式非法时拒绝。
- 两个 fake Adapter 可以同时注册、独立 probe、创建独立 Agent Runtime。
- 一个 Adapter missing/error 时，另一个 Adapter 对应的 Agent 仍然运行。
- inventory 和 heartbeat 同时携带每个已注册 Engine 的独立状态。

## 二、两阶段有界关闭

### 当前问题

Computer 收到 shutdown 后立即取消当前 Turn，再等待最多 15 秒。这实现的是“立即终止、等待清理”，不是已确认的“停止接收新工作，给当前 Turn 一个短暂完成窗口，超时后才终止”。

### 最终模块与接口

每个 Runner 使用两个不同信号：

1. `stop_requested`：停止 SSE、poll、Agenda 和新 Turn；已经进入 Engine 的 Turn 不接收这个信号。
2. `force_cancel`：只在优雅窗口耗尽时传给正在运行的 Engine，用于终止 Engine 进程。

Computer shutdown 顺序固定为：

1. 同时向所有 Runner 发出 `stop_requested`；
2. 已在执行的 Turn 继续自然完成，最长 15 秒；
3. 到期后向未完成 Runner 发出 `force_cancel`，终止 Engine 进程；
4. Server 在关闭数据库前，把当前 RuntimeSession 遗留的 `running` Run 标记为 `interrupted`；
5. Desktop 先回收 Computer，再关闭 Server，最后删除当前 runtime 目录。

15 秒是 Agent 可以继续工作的最长优雅窗口。之后只允许进行有界的进程回收和 Server 状态落盘，不能继续模型工作。

### 验收

- shutdown 期间不再开始新的 Turn。
- 一个能在窗口内完成的 active Turn 正常完成，不收到提前取消。
- 一个超过窗口的 Turn 被终止，Engine 进程组消失，Run 最终为 `interrupted`。
- 多个 Agent 共用同一个优雅 deadline，不按 Agent 串行累加 15 秒。
- 正常退出后 Server、Computer、Engine 和 runtime 目录均无残留。

## 三、业务 SQL 的模块所有权

### 当前问题

`agent_commands.rs` 同时负责结构化命令分派、active Run、durable inbox、消息插入、DM、HELD、delivery 和幂等请求，重新形成了近千行的依赖汇聚点；消息写入与 `messages.rs` 重复。

### 最终模块与接口

- `AgentCommands` 只负责一个深接口：`execute(claims, command)`。其实现负责事务编排和把领域结果映射成 protocol result，但不直接编写业务 SQL。
- `Messages` 拥有消息校验、序号推进、消息插入、glance、reply/HELD 和 DM 消息流程。
- `Rooms` 拥有 Direct Room 创建/复用及成员读取。
- `Runs` 拥有 active Run、Run inbox、delivery ack/action 和 session interruption。
- `CommandRequests` 拥有 Agent command 的幂等 reservation/result ledger。
- 不增加 Repository/Service/Manager 转发层；上述模块直接持有 SQL 和领域约束。

### 验收

- `agent_commands.rs` 不包含 `sqlx::query*`、数据库 Row 类型或消息插入实现。
- 消息正文校验和 `(room_id, sequence)` 推进只有一个实现。
- 现有结构化 AgentCommand、HELD、DM、Card、Climate 和幂等行为保持不变。
- architecture test 对新的 SQL ownership 形成约束，不能仅检查少数旧文件。

## 四、SSE 协议模块

### 当前问题

Desktop 直接导入 `computer::sse`，Desktop 与 Computer 又分别维护一套近似相同的解码、退避和重连循环。这既穿透了 Computer 门面，也会让重连规则发生分叉。

### 最终模块与接口

- SSE decoder 和通用 invalidation reconnect loop 归 `protocol::sse`，成为共享 protocol interface 的一部分。
- Desktop、Computer management、Agent Runner 只提供 endpoint、credential provider、event name 和收到 invalidation 后的动作。
- `computer::sse` 删除，不增加第四个 crate 对外入口。
- 三类连接都使用同一退避、解码、事件校验和取消语义。

### 验收

- Desktop 不再引用 `computer::*` 的实现模块。
- 重连循环只有一个实现。
- 有限 SSE stream 关闭后，Desktop、management 和 Agent 使用同一模块独立重连。
- 产品拓扑仍是一个 Desktop stream、一个 management stream、每个 active Agent 一个 stream；所有写操作仍走 HTTP。

## 五、故障验收收口

自动化测试必须区分“代码路径存在”和“故障真的发生过”：

- Redis：保留“启动时不可用、durable inbox 不丢失、Agenda fail closed”测试；新增“已连接后连接中断，再恢复订阅和 wake”的测试。
- 启动失败：覆盖 Server ready 失败、Computer ready 失败，并验证已启动子进程和 runtime 目录全部清理。
- Supervisor：在 Engine 正在执行时触发 shutdown，验证两阶段退出和 `interrupted` Run。
- SSE：共享 protocol 模块验证重连；Computer 测试验证 management 与 Agent 各自使用独立订阅，Supervisor/结构测试约束 Desktop 只启动一个订阅。
- 真实 OpenCode：保留显式 opt-in smoke。它会发起外部模型请求，未获得明确授权时不运行，也不能把“测试函数存在”写成“真实 smoke 已通过”。

## 完成条件

只有以下条件全部满足，`raw.md` 才能恢复“完成 41”的表述：

- 上述四个代码模块完成收口；
- Rust 全量测试、Desktop Rust 测试、前端测试、workspace check、Clippy 和格式检查通过；
- 故障测试不通过环境变量静默伪装为已执行；
- `raw.md` 准确区分已自动验证的能力与仍需授权的真实 OpenCode smoke。
