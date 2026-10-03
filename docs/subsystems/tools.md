# 工具

本页描述 Tool Call 怎样定义、选择与执行，以及七个内置工具的参数、上限与结果格式。`openwork-tools` 负责工具契约、内置工具、结果上限与文件变更记录。`openwork-core` 负责装配工具集、为每次调用盖章沙箱策略、审批与撤销。

读写边界、越界与审批见 [permissions.md](permissions.md)。Tool Call 的生命周期见 [session-runtime.md](session-runtime.md) §5。

## 1. 四层

| 层 | 类型 | 内容 | 生命周期 |
|---|---|---|---|
| 工具契约 | `Tool` | 稳定 ID、强类型输入与输出、schema、风险提示、执行逻辑 | 编译期 |
| 工具集 | `ToolsetConfig` → `FinalizedToolset` | Agent 从注册表中选出的子集 | Core 加载 Session 时构建 |
| 会话上下文 | `ToolSessionContext` | 工作目录、沙箱后端、环境变量、文件系统与进程后端、落盘目录、观察表 | 与工具集相同 |
| 调用上下文 | `ToolCallContext` | Tool Call ID、取消令牌、deadline、进度通道、本次的 `SandboxPolicy` | 每次调用 |

分层与强类型契约的理由见 [Agent Note：强类型契约与 FinalizedToolset](../../.agents/notes/implemented/architecture/2026-07-27-typed-tool-contract-and-finalized-toolset.md)。

## 2. 契约

```rust
#[async_trait]
pub trait Tool: Send + Sync + 'static {
    type Input: DeserializeOwned + JsonSchema + Send + 'static;
    type Output: ToolOutput;

    fn id(&self) -> ToolId;
    fn description(&self) -> &'static str;
    fn risk(&self) -> ToolRisk;
    fn inspect(&self, input: &Self::Input) -> CallInspection; // 默认：只读
    async fn execute(&self, session: &ToolSessionContext, call: ToolCallContext,
                     input: Self::Input) -> Result<Self::Output, ToolExecutionError>;
}
```

- `ToolId` 是显式写出的字符串，不从类型名推导。
- `serde` 用 `Input` 反序列化，`schemars` 用同一个类型生成 schema。生成时删去 `$schema` 与 `title`。
- `ToolDefinition` = `id` + `description` + `input_schema` + `risk_hint`。
- `ToolRisk` 有三个值：`ReadOnly`、`WorkspaceMutation`、`ProcessExecution`。Core 不按它做权限判定。
- `inspect` 报告执行前的事实（§6.1）。

注册表内部用 `ToolAdapter<T>` 把 `Tool` 擦除成 `DynTool`，以便放进 `HashMap<ToolId, …>`。Adapter 做三件事：

1. 把 JSON 反序列化成 `T::Input`。失败时返回 `invalid_arguments`，文本以 `invalid tool input:` 开头。
2. 把会话上下文与调用上下文分别传给 `execute`。
3. 把 `T::Output` 或 `ToolExecutionError` 转成 `ToolResult`（§8.1）。

## 3. 注册表与工具集

- `ToolRegistryBuilder` 登记当前二进制知道的工具。`builtin_registry()` 登记七个内置工具。
- `ToolsetConfig` 是 Agent 允许的工具 ID 列表。
- `finalize(config, session)` 生成 `FinalizedToolset`。

`finalize` 拒绝这些配置：空白 ID、空白描述、重复登记、重复选择、选择了未登记的工具。

`FinalizedToolset` 的不变量：

1. 广告给模型的定义与可调用的工具出自同一批选中项。
2. finalize 之后，工具不能增删或替换。它没有可变方法，字段私有。
3. `call` 只在已选中的工具中查找。找不到时返回 `tool_not_found` 结果。
4. 定义的顺序等于 `ToolsetConfig` 的顺序。
5. 会话上下文在 finalize 时绑定。Core 在每次调用时提供调用上下文。
6. 沙箱不可用时，schema 中没有越界参数（§7.5）。

| Agent | 工具集 |
|---|---|
| 默认 Agent | `read` `write` `edit` `grep` `glob` `list` `bash`，加上 Core 登记的 `conversation_history`（[compaction.md](compaction.md) §7.4） |
| explorer 子 Agent | `read` `grep` `glob` `list` `bash` |

根 Session 还有 Core 的控制工具。Core 的 `TurnToolset` 把它们加在工具集之后：`update_plan`（[update-plan.md](update-plan.md) §5.2）与子 Agent 工具（[multi-agent.md](multi-agent.md) §5）。控制工具不经过 `FinalizedToolset`，也不经过沙箱。

Skill 不是工具。模型用 `read` 读 `SKILL.md` 与 `references/`，用 `bash` 运行 `scripts/`。理由见 [Agent Note：Skill 复用 read](../../.agents/notes/implemented/architecture/2026-08-07-skills-use-read-instead-of-a-skill-tool.md)。

## 4. 会话上下文

```rust
pub struct ToolSessionContext {
    pub working_directory: PathBuf,
    pub sandbox: Arc<dyn SandboxBackend>,          // 自检结论；把 bash 的 argv 包进 sandbox-exec
    pub environment: Arc<HashMap<String, String>>,
    pub filesystem: Arc<dyn AsyncFileSystem>,
    pub process_backend: Arc<dyn ProcessBackend>,
    pub spill: Option<SpillDirectory>,             // §8.2
    pub observations: FileObservations,            // §7.4
    write_locks: …,                                // 每个真实路径一把异步写锁
}
```

- 它不保存沙箱模式、Turn 取消令牌、Tool Call ID、Permission Request、数据库连接或 Trace 记录器。沙箱模式随调用传递（§5）。
- `SandboxBackend` 来自 `openwork-sandbox`。它只包装 argv，不启动进程。`ProcessBackend` 启动进程（[permissions.md](permissions.md) §5–§6）。
- 内置工具经 `filesystem` 与 `process_backend` 访问文件和进程，不直接调用 `tokio::fs` 或 `tokio::process`。
- `ToolSessionContext::local` 从 OpenWork 进程的环境中只取 `PATH`、`HOME`、`SHELL`、`LANG`、`LC_ALL`、`TMPDIR`。
- Core 按 Session 持有观察表与落盘目录。重建工具集时，Core 传入同一份。
- `openwork-tools` 不依赖 `openwork-core`。

## 5. 调用上下文与取消

```rust
pub struct ToolCallContext {
    pub call_id: ToolCallId,
    pub cancel: CancellationToken,
    pub deadline: Option<Instant>,
    pub sandbox_policy: SandboxPolicy,
    progress: Option<mpsc::Sender<ToolProgress>>,
}
```

- Core 在每次调用前盖章 `sandbox_policy`：Session 当前的模式，加上用户为这一次批准的路径（[permissions.md](permissions.md) §4）。
- bash 用它生成 Seatbelt profile，文件工具用它做路径围栏（§6.2）。同一次调用里，两者读同一个值。
- 它不含 `SessionId`、`TurnId` 等 Core 类型。Core 的 Trace 维护 Session 与 Turn 的关联。

取消与终止：

- 一次模型响应中的 Tool Call 按顺序逐个执行。理由见 [Agent Note：串行执行 Tool Call](../../.agents/notes/implemented/architecture/2026-07-27-serial-tool-calls.md)。
- 每个 Tool Call 的取消令牌由当前 Turn 的令牌派生（`child_token`）。取消 Turn 即取消正在执行的调用。
- bash 的超时取 `timeoutMs` 与 `deadline` 剩余时间中较小的值。
- 超时与取消都进入 `TokioProcessBackend` 的同一个 `terminate`：向整个进程组发送 `SIGKILL`，再等待子进程退出。
- 进程组不存在（`ESRCH`）时视为已终止。其他 kill 失败或等待失败时，结果是 `outcome_unknown`。
- 子进程以 `kill_on_drop(true)` 启动，stdin 接到 `/dev/null`。
- 读取 stdout 与 stderr 的任务都会被 join。

## 6. 执行前检查与路径解析

### 6.1 执行前检查

`FinalizedToolset::prepare` 在执行前把工具报告的事实交给 Core。是否询问用户、是否执行，由 Core 决定（[permissions.md](permissions.md) §1）。

| `inspect` 返回 | 工具 | `PreparedCall` 中的事实 |
|---|---|---|
| `read_only` | `read` `grep` `glob` `list` | 无 |
| `writes(target)` | `write` `edit` | 写目标属于硬保护档时，`protected_target` 是规则拒绝文本 |
| `runs(command)` | `bash` | 命令原文；沙箱可用时，`danger` 是危险命令检测结果（[permissions.md](permissions.md) §10） |

- 带 `sandboxPermissions` 的调用，`escalation` 是规范化后的路径与理由。沙箱不可用时它总是 `None`。
- `prepare` 不推断命令会读写什么。命令实际做了什么，由内核在执行时判断（[permissions.md](permissions.md) §5、§7）。

### 6.2 路径解析

文件工具经 `ToolSessionContext::resolve_path(input, access, intent, policy)` 取得 `CheckedPath`：

1. 相对路径以 `working_directory` 为基准，变成绝对路径。
2. 按字面消去 `.` 与 `..`。
3. `MustExist`：规范化目标本身。目标不存在时报错。
4. `MayCreate`：规范化最近的已存在祖先，再接回其余部分。向上查找时遇到悬空的符号链接，就拒绝。
5. 用本次的 `SandboxPolicy::check(path, access, Actor::FileTool)` 判断真实路径。
6. 返回 `CheckedPath`。它的字段私有，只能由 `resolve_path` 构造。

- `check` 与 Seatbelt profile 由 `openwork-sandbox` 的同一组函数推导。`openwork-tools` 不持有路径规则（[permissions.md](permissions.md) §3、§4）。
- 读取不限于工作区。除凭据目录外，处处可读。
- 被拒绝时，错误文本就是给模型的拒绝标记（[permissions.md](permissions.md) §11）。硬保护档返回 `permission_denied`。敏感档、凭据档与工作区外返回 `permission_denied`，并标记 `sandbox_denied`。
- `write` 与 `edit` 先解析检查，再创建父目录，然后再解析检查一次。
- 原子写入在已检查的父目录下创建临时文件，再在同一目录内 `rename`。

这道围栏在进程内，不是内核边界。理由与残留风险见 [Agent Note：进程内围栏](../../.agents/notes/implemented/architecture/2026-08-01-file-tool-fence-in-process.md)。bash 的边界是 Seatbelt，见 [permissions.md](permissions.md) §5–§7。

## 7. 内置工具

七个工具分两组：`filesystem/{read, write, edit, grep, glob, list}` 与 `process/{bash}`。参数名用驼峰。

**每个工具自己保证结果有界。** 各工具的上限见 §9，超出部分的完整内容见 §8.2。理由见 [Agent Note：工具自己决定结果的形状](../../.agents/notes/implemented/architecture/2026-09-24-tool-owned-result-shapes.md)。

### 7.1 read

| 参数 | 默认 | 说明 |
|---|---|---|
| `path` | 必填 | 绝对路径，或相对工作目录的路径 |
| `offset` | 1 | 1 基起始行。传 0 时按 1 处理 |
| `limit` | 2000 | 1–2000。超出范围时返回 `invalid_arguments` |

- 输出每行为 `{行号}\t{内容}`。行尾的 `\r` 被去掉。
- 结果在三道上限中先到的那一道停止：行数 2000；内容 31,488 字节（32,000 减去 512 字节的结尾预留）；单行 2000 字符。
- 字节上限只停在最后一个完整行。**截断只发生在末尾。**
- 超过 2000 字符的行截断，并追加 `... (line truncated to 2000 chars)`。读取时每行只保留前 8,000 字节，其余部分边读边丢。
- 没读完时，末尾写明续读位置：

```text
[showing lines 2-3 of 4. Continue with offset=4]
[showing lines 1-812 of 3401; stopped at 32 KB. Continue with offset=813]
```

- 读到文件末尾时没有结尾行。空文件返回 `[empty file]`。
- 含 NUL 字节的行、无效 UTF-8 的行、目录、超过末尾的 `offset`，都返回说明下一步的错误文本。
- `read` 每次都读完整个文件：统计总行数，并计算整个文件的 SHA-256。成功的 `read` 在观察表中登记这个哈希（§7.4）。
- `read` 不落盘。

### 7.2 write

| 参数 | 说明 |
|---|---|
| `path` | 必填 |
| `content` | 必填，完整的文件内容，不超过 1 MiB（1,048,576 字节） |
| `sandboxPermissions`、`justification` | 越界参数（§7.5） |

- 目标不存在时创建它，并创建缺少的父目录。创建新文件不需要先读。
- 目标已存在时，必须先读过它（§7.4）。已存在的文件必须是不超过 1 MiB 的 UTF-8 文本。
- 写入用原子写入。提交前比较磁盘内容与读到的内容。不同时返回错误，不覆盖。
- 返回一行摘要。路径在工作区内时相对工作区。

```text
Created src/new.rs (+42)
Overwrote src/old.rs (+30 -12)
src/new.rs unchanged: the content is identical
```

- 内容有变化时，结果带一个 `file_change` Artifact（§8.4）。内容相同时没有 Artifact。

### 7.3 edit

| 参数 | 默认 | 说明 |
|---|---|---|
| `filePath` | 必填 | |
| `oldString` | 必填 | 要替换的原文。空字符串表示创建新文件 |
| `newString` | 必填 | 替换文本，或新文件的完整内容。不超过 1 MiB |
| `replaceAll` | `false` | 替换所有出现 |
| `sandboxPermissions`、`justification` | | 越界参数（§7.5） |

- 匹配是精确的字符串匹配。
- 以下情况失败：`oldString` 与 `newString` 相同；`oldString` 不存在；`oldString` 出现多次且没有设 `replaceAll`。
- 空 `oldString` 只创建新文件。目标已存在时失败。
- 修改已存在的文件前，必须先读过它（§7.4）。文件必须是不超过 1 MiB 的 UTF-8 文本。
- 返回一行摘要。行范围是替换后新内容所在的行：

```text
Created src/new.rs (+4)
Edited src/storage/time.rs:120-128 (+3 -1)
Edited src/lib.rs:5 (+1 -1)
Replaced 4 occurrences in src/lib.rs (+4 -4)
```

- 完整 diff 只进 `file_change` Artifact，不进模型可见的文本（§8.4）。

并发保护：

1. 取得该真实路径的异步写锁。`write` 也取同一把锁。
2. 读取当前内容，检查观察表。
3. 计算替换结果。
4. 原子写入在 `rename` 前再次比较磁盘内容。内容已变化时返回 `file changed while edit was being prepared`，不覆盖。

容错匹配是提议中的设计，见 [Agent Note：edit 容错匹配](../../.agents/notes/proposed/feature/2026-09-24-edit-tolerant-matching.md)。

### 7.4 先读后改

`FileObservations` 是一张内存中的表：真实路径 → 模型上次读到或写入的内容 SHA-256。

| 事件 | 观察表 |
|---|---|
| `read` 成功（读全文或其中一段都算） | 记录整个文件当前内容的哈希 |
| `write` / `edit` 成功 | 更新为写入后的哈希。连续编辑同一文件不需要重读 |
| `write` / `edit` 的目标已存在，但表中没有 | 拒绝：`Read <path> before editing it.` |
| 表中有，但磁盘内容的哈希不同 | 拒绝：`<path> changed since you last read it (by you via bash, or by the user). Read it again before editing.` |
| 目标不存在（创建新文件） | 不检查 |

- Core 按 Session 持有观察表，同一 Session 的所有 Turn 共用。
- 观察表不落库。进程重启后，模型要重新读目标文件。
- 撤销与重新应用（§8.4）不经过观察表。

理由见 [Agent Note：先读后改](../../.agents/notes/implemented/architecture/2026-09-24-read-before-edit.md)。

### 7.5 越界参数

沙箱可用时，`write`、`edit`、`bash` 的 schema 多出两个参数：

| 参数 | 取值 | 含义 |
|---|---|---|
| `sandboxPermissions.paths` | `[{path, access: read\|write, scope: exact\|subtree}]`，1–16 条 | 这一次调用额外需要的路径 |
| `justification` | 字符串 | 展示给用户的一句话理由。带 `sandboxPermissions` 时必须非空 |

- `SandboxPermissionsInput` 只有 `paths` 一个字段，并拒绝未知字段。
- 校验、卡片与授权的生存期见 [permissions.md](permissions.md) §9。
- 沙箱不可用时，`finalize` 从 schema 中删去这两个参数及只被它们引用的类型定义。

### 7.6 grep

| 参数 | 默认 | 说明 |
|---|---|---|
| `pattern` | 必填 | Rust regex 语法 |
| `path` | `.` | 搜索的目录或文件 |
| `glob` | 无 | 文件必须匹配的 glob，相对 `path` |
| `outputMode` | `content` | `content` \| `files_with_matches` \| `count` |

- 匹配用 `grep-regex` 与 `grep-searcher`，遍历用 `ignore`。不调用 `rg` 二进制。
- 遍历遵守 `.gitignore`，跳过隐藏文件与目录，不跟随符号链接。
- 文件含 NUL 字节时，按二进制文件停止搜索它。单行超过 64 MiB 时跳过该文件。读不了的文件也跳过。
- `glob` 相对搜索根匹配：`path: "desktop"` 下，`src/**/*.ts` 指 `desktop/src/…`。
- 结果中的路径在工作区内时相对工作区，可以直接交给 `read` 或 `edit`。
- 每个文件与每个匹配都检查取消与 30 秒超时。被取消时返回 `cancelled`。
- 上限是固定的，没有 `maxResults` 一类的参数。

`content` 模式按文件分组，每行为 `{行号}:{内容}`：

| 上限 | 值 |
|---|---|
| 返回的匹配行总数 | 250 |
| 每个文件 | 50 行。超出时追加 `(+N more matching lines in this file)` |
| 单行 | 2000 字节。超出时截断，并追加 `... (line truncated to 2000 bytes)` |

`files_with_matches` 与 `count` 模式最多列 250 个文件。

**达到上限后继续扫描，只计数，不保留内容。** 结果有省略时，结尾写准确总数与落盘路径：

```text
[showing 250 of 1834 matching lines in 97 files; full list saved to <path>]
[at least 1834 matching lines in 97 files; search timed out after 30s — narrow the path or glob]
```

- 落盘的完整列表每行为 `path:行号:内容`（`content`）、`path`（`files_with_matches`）或 `path:计数`（`count`）。
- 内存只保留返回的部分与计数。
- 没有匹配时返回 `No matches for /<pattern>/ in <path>`。

### 7.7 glob

| 参数 | 默认 | 说明 |
|---|---|---|
| `pattern` | 必填 | 相对 `path` 的 glob，例如 `**/*.rs` |
| `path` | `.` | 搜索的目录 |

- 遍历、路径显示、取消与 30 秒超时与 grep 相同。只匹配文件，不匹配目录。
- 最多返回 100 条，按修改时间倒序。没有修改时间的文件排在最后。
- 遍历时用容量 100 的堆保留最新的条目。内存与匹配总数无关。
- 超出上限时，结尾写准确总数，完整列表按遍历顺序落盘：

```text
[showing the 100 most recently modified of 150 files; full list saved to <path>]
[at least 150 files; search timed out after 30s — narrow the path or pattern]
```

- 没有匹配时返回 `No files match <pattern> in <path>`。

### 7.8 list

| 参数 | 默认 | 说明 |
|---|---|---|
| `path` | `.` | 目录 |
| `offset` | 0 | 0 基，作用于排序后的条目 |
| `limit` | 200 | 1–2000。超出范围时返回 `invalid_arguments` |

- 只列一层。条目按名称排序。目录名后加 `/`。条目类型只区分目录与非目录。
- 包含隐藏条目，不读 `.gitignore`。
- 后面还有条目时，结尾写 `[showing N entries from offset O; more entries available at offset M]`。

### 7.9 bash

| 参数 | 默认 | 说明 |
|---|---|---|
| `command` | 必填 | 交给 `/bin/bash -c` 的命令 |
| `timeoutMs` | 30,000 | 取值被限制在 1–120,000 |
| `sandboxPermissions`、`justification` | | 越界参数（§7.5） |

- 命令在 Seatbelt 沙箱内执行：`sandbox-exec -p <profile> -D … -- /bin/bash -c <command>`。本次调用的 `sandbox_policy` 生成 profile（[permissions.md](permissions.md) §5）。
- 工作目录是工作区根。每次调用都是新的 shell，`cd` 不保留。
- 环境变量只有 §4 列出的六个（有值时），再叠加 `SandboxEnvironment::bash_environment()` 的 `GOCACHE`。同名时后者覆盖。
- 沙箱不可用时不执行，返回 `sandbox_unavailable`（[permissions.md](permissions.md) §6）。
- 前台、单次调用。后台任务是提议中的设计，见 [Agent Note：bash 后台任务](../../.agents/notes/proposed/feature/2026-09-24-bash-background-jobs.md)。
- `workdir`、`description` 参数与 10 分钟超时是提议中的设计，见 [Agent Note：bash 的 workdir、description 与超时](../../.agents/notes/proposed/feature/2026-09-24-bash-workdir-description-timeout.md)。

输出：

- stdout 与 stderr 持续读取，按到达顺序合并为一路。进度通道仍分开两路（§8.3）。
- 内存只保留开头 2 KiB 与结尾 14 KiB。总量超过 16 KiB（16,384 字节）时，中间写明省略的字节数与落盘路径：

```text
... (41318 bytes omitted. Full output saved at ~/.openwork/spill/<session-id>/<tool-call-id>.txt — use read with offset/limit, or grep, to look at it.)
```

- 输出之后追加一行状态：

```text
[exit 7; duration 12 ms]
[timed out after 30000 ms; duration 30004 ms]
[cancelled; duration 51 ms]
```

| 结局 | `ToolResult` |
|---|---|
| 正常退出（含非零退出码） | `succeeded` |
| 超时 | `failed`，错误码 `timeout`，带终止前的输出 |
| 取消 | `cancelled`，带终止前的输出 |
| 启动失败、管道失败 | 执行错误（`execution_failed`） |
| `sandbox-exec` 报告自身失败 | `sandbox_unavailable` |
| 终止未确认 | `outcome_unknown` |

- **非零退出仍是成功的工具结果。**
- 拒绝识别作用在合并且截断后的输出上（[permissions.md](permissions.md) §7）。被内核拒绝时，结果标记 `sandbox_denied`，并在状态行后追加拒绝标记（[permissions.md](permissions.md) §11）。
- 结果不含任何网络限制或隔离的表述（[permissions.md](permissions.md) §5）。

## 8. 结果

### 8.1 结果结构

```rust
pub struct ToolResult {
    pub status: ToolResultStatus,          // succeeded | failed | denied | cancelled | outcome_unknown
    pub content: Vec<ToolResultContent>,   // 只有 Text
    pub artifacts: Vec<ToolResultArtifact>,
    pub error: Option<ToolError>,          // code、message、retryable
    pub sandbox_denied: bool,
}
```

- 错误码：`tool_not_found`、`invalid_arguments`、`permission_denied`、`cancelled`、`timeout`、`execution_failed`、`outcome_unknown`、`sandbox_unavailable`。
- 失败结果的文本等于错误消息。
- `sandbox_denied` 是结果上的事实，不是权限判定（[permissions.md](permissions.md) §1）。
- 模型适配器只发送文本。Artifact 不进模型上下文，也不计入 Token 预算。

结果的三条通道（文本、Artifact、Progress）的理由见 [Agent Note：结果的三条通道](../../.agents/notes/implemented/architecture/2026-07-27-tool-result-channels.md)。可操作的错误文本与重复调用提醒是提议中的设计，见 [Agent Note：可操作的错误与重复提醒](../../.agents/notes/proposed/feature/2026-09-24-actionable-tool-errors-and-repeat-warning.md)。

### 8.2 大结果落盘

完整内容在以下情况写入落盘文件：

| 情况 | 谁写 |
|---|---|
| bash 输出超过 16 KiB | bash，边读边写 |
| grep、glob 达到条数上限 | grep、glob，边扫描边写 |
| 其他任何结果的文本超过 32,000 字节 | `FinalizedToolset::call` 的兜底截断 |

- 兜底截断只保留开头。预算后半段有换行时，停在最后一个换行处；没有时，停在字符边界。
- 兜底截断的结尾写 `... (N bytes omitted. Full output saved at <path> — use read with offset/limit, or grep, to look at it.)`。
- 经 `FinalizedToolset` 执行的结果都不超过 32,000 字节，包括结尾的提示。控制工具（§3）不经过这一步。
- 位置：`~/.openwork/spill/<session-id>/<tool-call-id>.txt`。Session ID 与 Tool Call ID 中 `[A-Za-z0-9_-]` 以外的字符换成 `_`。
- 这个目录属于硬保护档：模型可读，不可写（[permissions.md](permissions.md) §3）。OpenWork 进程自己写入。
- **落盘失败时，结果照常返回，并去掉路径。** 结果不给出读不到的路径。
- 单个落盘文件不超过 64 MiB（`64 * 1024 * 1024` 字节）。写满即停，结果写明只保存了前 64 MB。
- grep 与 glob 的完整列表先在内存中缓冲 256 KiB，超出后改为流式写文件。没有省略时不留文件。
- bash 的输出在超过 16 KiB 时才建文件。执行失败时删除已写的部分。
- `read` 不落盘。
- Core 在删除 Session 时删除它的落盘目录。Core 启动时删除最后修改超过 7 天的目录。每个 Session 的落盘总量没有上限。

32,000 字节与 64 MiB 的理由见 [Agent Note：工具结果上限](../../.agents/notes/implemented/architecture/2026-09-24-tool-result-bounds.md)。旧结果的修剪见 [compaction.md](compaction.md) §1.1 与 [Agent Note：先修剪旧工具结果](../../.agents/notes/implemented/architecture/2026-09-24-prune-tool-results-before-summary.md)。

### 8.3 进度

```rust
pub enum ToolProgress {
    Stdout { chunk: String },
    Stderr { chunk: String },
    Message { message: String },
}
```

- `report_progress` 用 `try_send`。通道满或断开时，丢弃这一条，执行结果不变。
- Core 的进度通道容量为 64。Core 把进度转成 `tool_call_progress` Update，先于 `tool_call_finished` 发出。
- 进度不写入 Conversation，不进 Snapshot。断线重同步只恢复最终结果。
- 一次调用只产生一个最终 `ToolResult`。

### 8.4 文件变更 Artifact 与撤销

`write` 与 `edit` 改变文件内容时，结果带一个 `kind = "file_change"` 的 Artifact：

| 字段 | 内容 |
|---|---|
| `changeId` | Tool Call ID |
| `path` | 模型给的路径参数 |
| `kind` | `created` \| `modified` |
| `additions`、`deletions` | 准确的增删行数 |
| `hunks` | 带三行上下文的 diff hunk |
| `beforeHash`、`afterHash` | 前后内容的 SHA-256 |
| `beforeContent`、`afterContent` | 撤销与重新应用所需的内容 |
| `undone` | 是否已撤销 |

- Artifact 随 Tool 消息写入 `messages.content`，并在 `tool_call_finished` Update 中发给界面。
- 桌面端用 Artifact 显示 diff，不重新读磁盘。Session 重载后，仍显示当时的变更。
- diff 用 Myers 算法。轨迹超过 4,000,000 个单元时，改用公共前缀与后缀的简单 diff。
- 压缩运行状态的 `edited_paths` 只从这类 Artifact 得出（[compaction.md](compaction.md) §3）。

撤销与重新应用：

- 用户在界面上按变更 ID 操作。Session 有 Turn 在运行时，Core 返回 `SessionActive`。
- 写入经过文件工具围栏。策略是会话模式，加上每个涉及文件的精确写授权（[permissions.md](permissions.md) §4）。
- 撤销前，每个文件当前内容的哈希必须等于 `afterHash`；重新应用前，必须等于 `beforeHash`。**不相等时返回冲突，不覆盖。**
- 同一批中对同一路径的多次变更，撤销时按逆序执行，重新应用时按顺序执行。所以撤销“先创建、再编辑”会删除该文件。
- 中途失败时，回滚已完成的部分。回滚也失败时，返回 `RollbackFailed`。
- 成功后，Core 改写对应 Artifact 的 `undone`。
- 涉及的文件必须是不超过 1 MiB 的 UTF-8 文本。

这条链路不是数据库与文件系统的单一原子事务。文件已改写而 `undone` 写回失败时，磁盘与 Session 记录不一致。删除已创建的文件时，内容比较与删除之间有外部进程抢先写入的窗口。授权的理由见 [Agent Note：撤销授权](../../.agents/notes/implemented/architecture/2026-09-24-undo-reapply-grants.md)。

## 9. 上限

| 常量 | 值 | 位置 | 来源 |
|---|---|---|---|
| 结果文本上限 | 32,000 字节，含结尾 | `spill.rs` `MAX_RESULT_BYTES` | OpenWork：8,000 token × 4 字节，与请求投影的单条上限对齐。DSH 是 50 KiB（`packages/fs/tool-fs/src/read-render.ts` `READ_MAX_BYTES`） |
| 结尾预留 | 512 字节 | `spill.rs` `FOOTER_RESERVE_BYTES` | OpenWork |
| 单个落盘文件 | 64 MiB | `spill.rs` `MAX_SPILL_BYTES` | OpenWork |
| 落盘前的内存缓冲（grep、glob） | 256 KiB | `spill.rs` `SPILL_BUFFER_BYTES` | OpenWork |
| 落盘保留期 | 7 天 | `openwork-core/src/spill.rs` `SPILL_RETENTION` | OpenWork |
| read 行数 | 2000 | `read.rs` `MAX_LINES` | DSH `packages/fs/tool-fs/src/read.ts` `READ_LIMIT` |
| read 单行 | 2000 字符 | `read.rs` `MAX_LINE_CHARS` | DSH `packages/fs/tool-fs/src/read-render.ts` `READ_MAX_LINE_LENGTH` |
| write 内容、edit 文件与 `newString` | 1 MiB | `write.rs`、`edit.rs` `MAX_BYTES` | OpenWork |
| grep 匹配行 | 250 | `grep.rs` `MAX_LINES` | DSH `packages/fs/tool-fs-search/src/grep.ts` `GREP_MAX_MATCHES` |
| grep 每个文件 | 50 行 | `grep.rs` `MAX_LINES_PER_FILE` | maka `packages/runtime/src/grep-search.ts` `GREP_MAX_LINES_PER_FILE` |
| grep 单行 | 2000 字节 | `grep.rs` `MAX_LINE_BYTES` | DSH `packages/fs/tool-fs-search/src/grep.ts` `GREP_MAX_LINE_BYTES` |
| grep 文件模式 | 250 个文件 | `grep.rs` `MAX_FILES` | OpenWork |
| glob 结果 | 100 | `glob.rs` `MAX_RESULTS` | DSH `packages/fs/tool-fs-search/src/glob.ts` `GLOB_MAX_RESULTS` |
| grep、glob 超时 | 30 秒 | `scan.rs` `SCAN_TIMEOUT` | DSH `packages/fs/tool-fs-search/src/search-core.ts` `SEARCH_TIMEOUT_MS` |
| list 每页 | 默认 200，最大 2000 | `list.rs` | OpenWork |
| bash 输出开头 | 2 KiB | `backend/process.rs` `CAPTURE_HEAD_BYTES` | OpenWork。DSH 只保留尾部（`packages/subprocess/subprocess-local/src/output.ts`） |
| bash 输出结尾 | 14 KiB | `backend/process.rs` `CAPTURE_TAIL_BYTES` | OpenWork |
| bash 超时 | 默认 30 秒，最大 120 秒 | `bash.rs` | OpenWork |

位置一列的路径相对 `crates/openwork-tools/src/`，另有写明的除外。DSH 与 maka 的路径相对各自仓库根。

## 10. 验收

编号沿用原设计文档，代码与测试注释按这些编号引用。测试路径相对 `crates/`，前端测试写出文件与用例名。

`openwork-tools/tests/sandbox_calls.rs` 只在 macOS 上编译。`read_before_edit.rs`、`skill_paths.rs`、`file_changes.rs` 使用真实的 Seatbelt 自检。带 Postgres 的测试需要 `TEST_DATABASE_URL`。

第 29–30、35–37、43 条不在本节。它们的验收条件在 §7.3、§7.9、§8.1 链接的 proposed Agent Note 中。

### 契约与分层

1. 广告给模型的定义与可调用的工具来自同一个 `FinalizedToolset`。
   - 测试：`openwork-tools/src/registry.rs::finalized_toolset_is_the_model_and_dispatch_subset`
2. finalize 之后，工具不能增删或替换。
   - 状态：无测试。`FinalizedToolset` 的字段私有，没有可变方法（`openwork-tools/src/registry.rs`）。
3. Input Schema 与反序列化类型来自同一个 Rust 类型。
   - 测试：`openwork-tools/src/registry.rs::typed_input_drives_schema_and_validation`
4. 工具集之外的工具既不广告，也不执行；调用它得到 `tool_not_found` 结果，Turn 继续。
   - 测试：`openwork-tools/src/registry.rs::finalized_toolset_is_the_model_and_dispatch_subset`；`openwork-core/src/session_tools.rs::the_explorer_exposes_only_the_read_only_role_surface`；`openwork-core/tests/session_runtime.rs::unknown_tool_becomes_a_result_and_the_model_continues`
5. 输入不变时，工具定义的顺序确定，等于 `ToolsetConfig` 的顺序。
   - 测试：`openwork-tools/src/builtins/mod.rs::builtin_registry_exposes_each_tool_once_in_selected_order`
6. `openwork-tools` 不依赖 `openwork-core`。
   - 状态：手动：`crates/openwork-tools/Cargo.toml` 中没有 `openwork-core`。`openwork-core` 依赖 `openwork-tools`，反向依赖会形成环，cargo 拒绝编译。

### 路径安全

7. 写入经符号链接落到当前策略的可写范围之外时，拒绝写入；读取经符号链接进入凭据目录时，拒绝读取；读取其他位置不受限。
   - 测试：`openwork-tools/src/builtins/filesystem/write.rs::rejects_new_file_through_symlink_outside_workspace`；`openwork-tools/src/builtins/filesystem/write.rs::rejects_protected_metadata_through_symlink_alias`；`openwork-tools/src/builtins/filesystem/read.rs::rejects_read_through_symlink_into_a_credential_directory`；`openwork-tools/tests/skill_paths.rs::an_alias_cannot_bypass_canonical_skill_root_write_protection`
8. 新建文件时，父目录越界则拒绝；路径中有悬空符号链接时拒绝。
   - 测试：`openwork-tools/src/builtins/filesystem/write.rs::rejects_new_file_through_symlink_outside_workspace`；`openwork-tools/src/builtins/filesystem/write.rs::rejects_new_file_through_dangling_symlink`
9. 内置文件工具都先经 `resolve_path` 取得 `CheckedPath`，再用它的路径访问文件系统后端。
   - 状态：手动：检索 `crates/openwork-tools/src/builtins/filesystem/`，每个 `session.filesystem` 调用的路径都来自 `CheckedPath::as_path()`，或来自对已检查根目录的遍历。`AsyncFileSystem` 的方法接受 `&Path`，类型系统不阻止绕过。
10. 越界批准只改变这一次调用的 `sandbox_policy`，不能解开硬保护路径。
    - 测试：`openwork-tools/tests/sandbox_calls.rs::acc_10_an_escalation_widens_only_this_call_and_never_hard_protected_paths`；`openwork-tools/tests/skill_paths.rs::write_and_edit_are_denied_for_the_agents_skill_root_in_every_mode`；`openwork-sandbox/src/policy.rs::grants_unlock_what_they_name_but_never_hard_protected_paths`
    - 缺口：带写授权的文件工具调用没有经工具层测试；工具层只对 bash 断言了“授权不解开硬保护”。

10a. 同一组路径在文件工具围栏与 Seatbelt profile 下得到相同的可读、可写结论。
   - 测试：`openwork-sandbox/tests/parity.rs::acc_10_file_tool_fence_and_seatbelt_agree_on_every_path`

10b. 凭据目录对 `read`、`grep`、`glob`、`list` 与 bash 同样不可读。
   - 测试：`openwork-tools/tests/sandbox_calls.rs::acc_10b_credential_directories_are_unreadable_for_file_tools_and_bash`；`openwork-tools/src/builtins/filesystem/read.rs::rejects_read_through_symlink_into_a_credential_directory`
   - 缺口：只断言了 `read` 与 bash；`grep`、`glob`、`list` 经同一个 `resolve_path`，没有单独测试。

### 沙箱

10c. bash 的进程树在 Seatbelt 沙箱内执行，profile 来自本次调用的 `sandbox_policy`。
   - 测试：`openwork-tools/tests/sandbox_calls.rs::acc_10c_bash_runs_under_the_policy_of_this_call`

10d. 沙箱自检失败时，bash 不执行，返回 `sandbox_unavailable`；代码中不存在不经 Seatbelt 启动 bash 的路径。
   - 测试：`openwork-tools/tests/sandbox_calls.rs::acc_10d_bash_does_not_run_when_the_sandbox_is_unavailable`；`openwork-tools/src/prepare.rs::an_unavailable_sandbox_reports_no_dangerous_command`
   - 缺口：“不存在其他启动路径”靠检索确认：bash 唯一的启动点在 `bash.rs`，经 `SandboxBackend::wrap`。

10e. 内核拒绝的调用在结果上标记 `sandbox_denied`，并附拒绝标记；`sandbox-exec` 启动失败时不标记。
   - 测试：`openwork-tools/tests/sandbox_calls.rs::acc_10e_kernel_denials_are_marked_with_the_escalation_hint`；`openwork-tools/tests/sandbox_calls.rs::acc_10e_a_runner_failure_is_unavailable_not_denied`

10f. 沙箱不可用时，schema 中不出现 `sandboxPermissions` 与 `justification`。
   - 测试：`openwork-tools/src/builtins/mod.rs::escalation_parameters_disappear_when_the_sandbox_is_unavailable`

### 取消与进程

11. 取消 Turn 时，正在执行的 Tool Call 被取消，Turn 以取消结束。
    - 测试：`openwork-core/tests/session_runtime.rs::cancelling_a_turn_cancels_the_active_tool_call`；`openwork-tools/src/builtins/process/bash.rs::cancellation_preserves_partial_output`
12. 超时与用户取消走同一条终止路径。
    - 测试：`openwork-tools/src/backend/process.rs::process_backend_enforces_timeout`；`openwork-tools/src/backend/process.rs::process_backend_terminates_a_cancelled_process_group`
    - 缺口：两个测试分别断言超时与取消的结局，没有断言两者调用同一个 `terminate`。这一点由代码结构保证。
13. bash 终止整个进程组，并返回终止前已收集的输出。
    - 测试：`openwork-tools/src/backend/process.rs::process_backend_terminates_a_cancelled_process_group`；`openwork-tools/src/builtins/process/bash.rs::timeout_preserves_partial_output`；`openwork-tools/src/builtins/process/bash.rs::cancellation_preserves_partial_output`
14. kill 失败或终止无法确认时，结果是 `outcome_unknown`。
    - 状态：无测试。
15. `yes` 这类无限输出的命令不会耗尽内存，也不会写满磁盘。
    - 测试：`openwork-tools/src/backend/process.rs::unbounded_output_stays_bounded_in_memory_and_on_disk`

### 工具语义

16. 以下情况 `edit` 失败：`oldString` 不存在；出现多次且没有设 `replaceAll`；与 `newString` 相同。
    - 测试：`openwork-tools/src/builtins/filesystem/edit.rs::rejects_ambiguous_or_noop_edits`
    - 缺口：`oldString` 不存在的情况没有测试。
17. 并发修改使文件内容变化时，`edit` 返回错误，不覆盖文件。
    - 测试：`openwork-tools/src/backend/filesystem.rs::atomic_write_rejects_stale_content_without_overwriting`
    - 缺口：只在后端层断言；没有经 `edit` 工具的并发测试。
18. `read` 与 `list` 的分页在稳定排序上进行。
    - 测试：`openwork-tools/src/builtins/filesystem/read.rs::reads_a_one_based_page_with_original_line_numbers`；`openwork-tools/src/builtins/filesystem/list.rs::lists_a_zero_based_sorted_page`
19. bash 非零退出是成功的工具结果，不是基础设施错误。
    - 测试：`openwork-tools/src/builtins/process/bash.rs::non_zero_exit_is_a_completed_tool_result`

### 有界结果

20. `read` 不带参数读一个 5000 行的文件时，返回第 1–2000 行，或到 32,000 字节以内的最后一个完整行，末尾写明 `Continue with offset=N`；不存在从中间截断的输出。
    - 测试：`openwork-tools/src/builtins/filesystem/read.rs::acc_20_default_read_stops_at_the_first_limit_and_says_how_to_continue`
21. 一个 10,000 字符的单行截断到 2000 字符，并带标注。
    - 测试：`openwork-tools/src/builtins/filesystem/read.rs::acc_21_long_lines_are_cut_to_2000_chars_and_marked`；`openwork-tools/src/builtins/filesystem/read.rs::overlong_lines_are_cut_while_streaming`
22. `grep` 返回不超过 250 行、单个文件不超过 50 行、单行不超过 2000 字节，结尾报告准确的匹配行数与文件数；内存占用与总匹配数无关。
    - 测试：`openwork-tools/src/builtins/filesystem/grep.rs::acc_22_bounds_lines_and_reports_exact_totals`；`openwork-tools/src/builtins/filesystem/grep.rs::file_modes_report_exact_totals`
    - 缺口：没有测试断言内存占用；它由实现方式保证（只保留返回的部分与计数）。
23. `grep` 与 `glob` 超过 30 秒时，返回已扫描部分，并写明“至少”。
    - 测试：`openwork-tools/src/builtins/filesystem/grep.rs::acc_23_timeout_reports_at_least_the_counted_matches`；`openwork-tools/src/builtins/filesystem/glob.rs::timeout_reports_at_least_the_counted_files`
    - 缺口：两个测试只检查结尾文本的生成，没有真实运行 30 秒的扫描。
24. `glob` 返回不超过 100 条，按修改时间倒序，报告准确总数。
    - 测试：`openwork-tools/src/builtins/filesystem/glob.rs::acc_24_returns_the_newest_hundred_with_an_exact_total`
25. bash 输出超过 16 KiB 时，模型看到开头 2 KiB 与结尾 14 KiB，中间标注省略的字节数。
    - 测试：`openwork-tools/src/builtins/process/bash.rs::acc_25_long_output_keeps_head_and_tail_and_spills_the_rest`；`openwork-tools/src/backend/process.rs::keeps_a_short_head_and_a_long_tail`；`openwork-tools/src/backend/process.rs::merges_stdout_and_stderr`
26. 被截断的结果（bash 超过 16 KiB、grep 与 glob 达到条数上限、其他结果超过 32,000 字节）完整写入 `~/.openwork/spill/<session-id>/`。模型看到的文本附带该路径，且能用 `read` 读到它；沙箱内的 bash 不能写这个目录。
    - 测试：`openwork-tools/src/builtins/process/bash.rs::acc_25_long_output_keeps_head_and_tail_and_spills_the_rest`；`openwork-tools/src/builtins/filesystem/grep.rs::acc_22_bounds_lines_and_reports_exact_totals`；`openwork-tools/src/builtins/filesystem/glob.rs::acc_24_returns_the_newest_hundred_with_an_exact_total`；`openwork-tools/src/spill.rs::oversized_results_are_cut_at_the_end_and_saved`；`openwork-core/src/session_tools.rs::acc_26_spilled_output_is_readable_and_never_writable`；`openwork-tools/tests/sandbox_calls.rs::acc_26_bash_cannot_write_the_spill_directory`
27. 落盘失败时，结果仍然返回，且不出现指向不存在文件的路径。
    - 测试：`openwork-tools/src/spill.rs::failed_spill_omits_the_path`；`openwork-tools/src/backend/process.rs::an_abandoned_capture_leaves_no_spill_file`

### 编辑

28. `write` 与 `edit` 返回一行摘要，例如 `Edited <path>:<起>-<止> (+a -d)`，行范围是新内容所在的行；完整 diff 只出现在 Artifact。
    - 测试：`openwork-tools/src/builtins/filesystem/write.rs::acc_28_write_reports_one_line`；`openwork-tools/src/builtins/filesystem/edit.rs::acc_28_edits_report_one_line_with_the_new_line_range`；`openwork-tools/src/builtins/filesystem/edit.rs::replace_all_reports_the_number_of_occurrences`
31. 对没读过的已存在文件，`edit` 与 `write` 拒绝修改，并提示先读。
    - 测试：`openwork-tools/tests/read_before_edit.rs::acc_31_unread_existing_files_cannot_be_edited_or_overwritten`
32. 对读过之后被 bash 或用户改过的文件，`edit` 与 `write` 拒绝修改，并提示重读。
    - 测试：`openwork-tools/tests/read_before_edit.rs::acc_32_files_changed_since_the_read_must_be_read_again`
    - 缺口：测试只断言了 `edit`；`write` 经同一个 `check_current`。
33. 连续两次 `edit` 同一文件，第二次不需要重读。
    - 测试：`openwork-tools/tests/read_before_edit.rs::acc_33_consecutive_edits_do_not_need_a_new_read`；`openwork-tools/tests/read_before_edit.rs::observations_carry_across_toolsets_that_share_a_table`
34. 创建新文件不需要先读。
    - 测试：`openwork-tools/tests/read_before_edit.rs::acc_34_creating_files_needs_no_read`

### 结果与 Artifact

38. 模型上下文中不出现 Artifact，Token 统计不包含它。
    - 测试：`openwork-models/src/adapters/openai_chat/request.rs::tool_result_artifacts_are_not_sent_to_the_provider`；`openwork-core/src/context/budget.rs::tool_result_artifacts_do_not_count_toward_the_conversation`
    - 缺口：只有 OpenAI Chat 适配器有测试；`anthropic_messages` 与 `openai_responses` 没有。
39. 进度发送失败不改变工具结果。
    - 测试：`openwork-tools/src/context.rs::disconnected_progress_consumer_does_not_fail_the_tool_call`；`openwork-core/tests/session_runtime.rs::tool_progress_is_forwarded_before_the_terminal_tool_update`
    - 缺口：测试只断言断开的通道不让调用失败，没有比较有无进度时的结果。
40. Session 重载后，仍能显示当时的准确 diff。
    - 测试：`openwork-core/tests/postgres_session_storage.rs::postgres_storage_round_trips_a_complete_tool_turn`；`openwork-core/tests/session_runtime.rs::tool_result_artifacts_are_persisted_in_messages_and_forwarded_live`；`desktop/src/features/chat/components/FileChangeCard.test.tsx › "renders the exact hunk used by an expanded file activity"`
    - 缺口：没有端到端的重载测试；三个测试分别覆盖落库、转发与按 Artifact 渲染。
41. 撤销与重新应用在哈希不匹配时返回冲突，不覆盖文件。
    - 测试：`openwork-tools/tests/file_changes.rs::undo_refuses_to_overwrite_an_external_change`；`openwork-tools/tests/file_changes.rs::reapply_refuses_to_overwrite_a_change_made_after_undo`
42. 同批次的多次变更，撤销时按逆序执行，重新应用时按顺序执行。
    - 测试：`openwork-tools/tests/file_changes.rs::undo_reverses_multiple_changes_to_the_same_file_in_reverse_order`；`openwork-tools/tests/file_changes.rs::reapply_restores_chained_changes_in_forward_order`
