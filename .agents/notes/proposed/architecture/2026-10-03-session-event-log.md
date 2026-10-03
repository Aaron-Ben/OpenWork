# Agent Note: 会话存为只追加的事件日志

Status: proposed

## 问题

一个 Session 的事实分散在 Postgres 的多张表中：`sessions`、`turns`、`messages`、`conversation_compactions`、`turn_plans`。OpenWork 是单机桌面应用（用户决定，2026-10-03），这带来几个问题：

- 用户要先启动 Postgres 服务才能使用工作台（[local-postgres.md](../../../../docs/local-postgres.md)）。
- 读写一次会话要跨多张表，存储层的 Record 类型还泄漏到了 Desktop。
- 库里的时间按东八区墙上时间存储。为此，代码与迁移中有 98 处 `AT TIME ZONE`、46 处 `to_char`，以及 3 份 `CHINA_OFFSET`。
- 25 个测试文件依赖 `TEST_DATABASE_URL`，没有设置时静默返回。

参照项目的做法一致：只追加的日志是唯一事实来源，索引与视图从日志派生。

- pi：JSONL 树，路径 `~/.pi/agent/sessions/`（`packages/coding-agent/src/core/session-manager.ts`）。
- DSH：JSONL 事件日志，Zstandard 帧加 fsync 与单写者租约（`packages/session/session-persistence-jsonl/`）。SQLite 只做全文索引，而且默认关闭。
- Codex：JSONL rollout 是权威历史，SQLite 存可查询的元数据。SQLite 写失败不影响 JSONL 写入（`codex-rs/thread-store/README.md`，测试 `sqlite_failure_does_not_fail_durable_jsonl_write`）。

## 提议

### 事件日志

- 每个 Session 一个目录：`~/.openwork/sessions/<session-id>/`。日志文件为 `events.v1.jsonl`，只追加。
- 首行是头部：格式版本、session id、父 Session、创建时刻、工作区、Agent。
- 其余每行是一个事件。初版事件类型取自现有的表：

| 现在的表或字段 | 事件 |
|---|---|
| `turns` 的开始与结束 | `turn_started`、`turn_finished`（含状态与错误码） |
| `messages`（含 `message_kind`） | `message`（role、kind、内容块） |
| Tool Call 与 Tool Result | `tool_call`、`tool_result` |
| `conversation_compactions` | `compaction`（摘要、提醒、替换到哪条事件） |
| `turn_plans` | `plan_updated` |
| `sessions.sandbox_mode` | `mode_changed` |
| 文件改动与撤销 | `file_change`、`file_change_reverted` |

- 内存中的会话状态与 Chat State 都由回放日志得到。
- Session actor 是唯一写者。每批追加后调用 fsync。用文件锁防止两个进程同时写同一个 Session。

### 崩溃恢复

- 读取时，截掉最后一行不完整的内容。
- 没有 `turn_finished` 的 Turn，在加载时补写 `turn_finished`，状态为 `interrupted`。它取代现在启动时的 `mark_running_interrupted`。

### 格式版本

- 头部带格式版本，读取时遇到未知事件就拒读。
- 按 CLAUDE.md“不保留向后兼容”，不写版本迁移。版本不匹配的会话拒绝打开，并说明原因。

### 旧数据

不迁移 Postgres 中的现有数据（用户决定，2026-10-03）。切换后，旧会话不可见。

### 派生数据

- `~/.openwork/openwork.sqlite`：会话列表、标题、最近活动与全文检索。删除后可以从日志重建。写入失败只记 warn，不影响日志写入。
- Trace 写入 `~/.openwork/trace.sqlite`。Trace 带有请求正文，不能从会话日志派生；但 [trace.md](../../../../docs/subsystems/trace.md) 规定 Trace 可以丢、业务不受影响，所以它可以独立于会话日志存储。
- 附件与落盘结果已经是文件，不变。

### 时间

照 DSH 的做法：

- 所有时刻存为 Unix 毫秒整数（`i64`）。JSONL 中写成数字，SQLite 中是 `INTEGER` 列。DSH 的会话事件字段 `time` 是“Unix epoch milliseconds”（`packages/core/session/src/types.ts`），写入时取 `Date.now()`（`packages/core/session/src/index.ts`）。
- 界面按用户本机的时区与语言显示。DSH 用 `new Date(t).toLocaleTimeString(undefined, …)`（`packages/client/ui-trajectory/src/client/TrajectoryTimeline.tsx`）。
- 只有按墙上时间触发的功能（例如定时任务）才另存 IANA 时区名。DSH 的 schedule 记录带 `timeZone`（`packages/schedule/schedule/src/types.ts`）。OpenWork 现在没有这类功能。
- 东八区墙上时间的约定、`CHINA_OFFSET` 与 `desktop/src/lib/dateTime.ts` 中固定 `Asia/Shanghai` 的显示，全部删除。

### SQLite 驱动

建议用 `sqlx` 的 SQLite 后端：工作区已经依赖 `sqlx`，可以保留异步接口与编译期查询检查。另一个选择是 `rusqlite`，它是同步接口，需要放进 `spawn_blocking`。

## 考虑过的方案

**继续使用 Postgres。** 没有采用：单机桌面没有多进程并发写的需求，却要用户安装并启动一个数据库服务。

**全部存进 SQLite，不用 JSONL。** OpenCode 在 2026-02 从 JSON 文件迁到了 SQLite（提交 `6d95f0d`）。没有采用：事实与索引混在一起，日志不能直接阅读与追加审计，崩溃时也难以判断哪些写入已经生效。

**pi 的 JSONL 树（条目带 `parentId`，同一文件内分支）。** 本提议不采用：OpenWork 现在用 compaction 与 rewind 处理历史，没有分支的需求。以后需要分支时，再给事件加上 `parent_id`。

## 验收条件

- 工作台不依赖任何数据库服务即可运行。
- 删除 `openwork.sqlite` 后重启，会话列表与检索从日志完整重建。
- 在 Turn 运行中用 `kill -9` 结束进程。重启后会话能打开，未结束的 Turn 状态为 `interrupted`。
- 会话日志的最后一行被截断时，会话仍能打开，只丢失被截断的那一行。
- `openwork-core` 的测试不再需要 `TEST_DATABASE_URL`，也没有静默跳过的测试。
- 代码中不再有 `AT TIME ZONE`、`to_char` 与 `CHINA_OFFSET`。

## 风险

- 存储层要整体重写，`session/storage.rs` 与 `storage/postgres/` 都受影响。
- 上下文检查与 Trace 列表要改为读日志与 SQLite，需要确认性能。
- 会话日志只增不减，长会话的文件会变大。需要观察大小，必要时再加压缩（DSH 用 Zstandard 帧）。
