# 工具

工具运行时是四层结构，收敛于 `FinalizedToolset`。本文覆盖契约、权限边界、路径安全和七个内置工具的语义。

## 1. 四层

| 层 | 是什么 | 生命周期 |
|---|---|---|
| **工具契约** | 稳定 ID、typed input/output、schema、风险等级、执行逻辑 | 编译期 |
| **工具集选择** | Agent 从已注册工具中选出真正可用的子集 | Agent 构建时 |
| **`ToolSessionContext`** | 工作目录、权限档案、文件系统与进程后端、环境 | 与 Session 同寿 |
| **`ToolCallContext`** | 本次调用的 ID、取消令牌、deadline、进度通道 | 每次调用 |

分层的作用是让每个依赖只出现在它真正稳定的那一层：工作目录一个 Session 内不变，取消令牌每次调用都不同——混在一起会导致要么泄漏跨调用状态，要么每次调用重建整个上下文。

## 2. 第一层：工具契约

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

- `ToolId` 是**稳定的内部身份**，不从文件名或类型名隐式推导；
- Input 由 `serde` 反序列化，Schema 由 `schemars` 从**同一个类型**生成——两者不可能不一致；
- Tool Definition = `id + description + schema + risk`；
- handler **不再手工 `Value::get("...")`**。

### object-safe 分发

带关联类型的 `Tool` 不能直接作为 `dyn Tool` 保存，Registry 内部用 `ToolAdapter<T>` 做一次类型擦除：

```rust
#[async_trait]
trait DynTool: Send + Sync {
    fn definition(&self) -> ToolDefinition;
    fn risk(&self) -> ToolRisk;
    async fn call(&self, session: Arc<ToolSessionContext>,
                  call: ToolCallContext, input: Value) -> ToolResult;
}
```

Adapter 负责：JSON → `T::Input` 反序列化；把两级上下文分别传入；把 `T::Output` 或 `ToolError` 统一转成 `ToolResult`。**具体工具保持强类型，运行时仍可 `HashMap<ToolId, Arc<dyn DynTool>>` 分发。**

## 3. 第二层：Registry 与 Toolset

```text
Registry = 当前二进制知道哪些工具
Toolset  = 当前 Agent 实际允许哪些工具
```

两者必须分开：Registry 是编译期事实，Toolset 是 Agent 策略。

## 4. 第三层：会话级上下文

```rust
pub struct ToolSessionContext {
    pub working_directory: PathBuf,
    pub permissions: PermissionProfile,
    pub environment: Arc<HashMap<String, String>>,
    pub filesystem: Arc<dyn AsyncFileSystem>,
    pub process_backend: Arc<dyn ProcessBackend>,
}
```

**它不保存**：Turn 取消令牌、Tool Call ID、当前 Permission Request、SessionActor、Chat State、ModelPort、数据库连接、Trace Recorder。

Tools 可以依赖 `ToolSessionContext`，但**不能反向依赖 `openwork-core`**。

### 为什么注入文件系统和进程后端

内置工具不直接调 `tokio::fs` / `tokio::process`，而是走两个受控后端：

- 单元测试不需要访问真实工作目录；
- **路径检查可以在唯一入口强制执行**；
- 进程表、kill、timeout 和取消语义集中管理；
- 进程启动方式集中在一处，替换实现不必改每个工具。

## 5. 第四层：调用级上下文

```rust
pub struct ToolCallContext {
    pub call_id: ToolCallId,
    pub cancel: CancellationToken,
    pub deadline: Option<Instant>,
    pub progress: Option<ProgressSender>,
}
```

**不放入** `SessionId` / `TurnId` 等 Core 类型，避免反向依赖。Session/Turn 关联由 Core 的 Trace 层维护。

### 取消不变量

- Session 创建时**不**创建永不更新的工具取消令牌；
- 每个 Tool Call 从当前 Turn 的 token 派生自己的 child token；
- 用户取消 Turn 时所有活动调用同时取消；
- timeout 与用户取消最终进入**同一条**进程终止路径；
- 不允许把持有 child token 的 future 丢进 `tokio::spawn` 后丢弃 JoinHandle；
- **ProcessBackend 不得遗留失去 owner 的子进程**；kill 确认失败时必须继续持有句柄并明确返回 `outcome_unknown`；
- `kill_on_drop` 只是最后防线，不能代替显式取消协议。

## 6. FinalizedToolset

四层的收敛点：

```rust
pub struct FinalizedToolset {
    definitions: Vec<ModelToolDefinition>,
    tools: HashMap<ToolId, Arc<dyn DynTool>>,
    session: Arc<ToolSessionContext>,
}
```

**不变量：**

1. `definitions` 与 `tools` 用**同一批** selected entries 构造；
2. finalize 后不可增删替换工具；
3. `call` 只能在 finalized map 中查找；
4. schema、风险和执行 adapter 来自同一个注册项；
5. Session Context 在 finalize 时绑定，Call Context 每次调用由 Core 提供；
6. Core 只持有一个 `Arc<FinalizedToolset>`，不再同时持有 Catalog 和 Executor。

第 1 条是"广告了但调不动 / 能调但没广告"的根本防线。

V1 没有动态 MCP，因此不需要 `ToolBridge`。若未来出现动态工具，在不可变的内置 toolset 之外**增加独立动态层**，而不是让当前对象提前变可变。

## 7. 权限：两类，不能混

### 7.1 用户决策 —— Core 负责

```text
Allow → 立即执行
Ask   → 发出 PermissionRequest，等待用户
Deny  → 返回 denied，不调用工具
```

权限等待会修改 Session 运行时状态、发 Live Update、接收 Desktop 命令，所以它属于 `SessionActor` 的控制流。

### 7.2 执行期强制 —— Tools 负责

目标路径是否在允许范围内、是否允许写保护目录、进程参数是否合法、symlink 是否越界。

> **用户选择 `Allow` 不能绕过这些边界。**

分工：工具声明它这次会产生什么**效果**，权限系统按效果决定问不问（[permissions.md §2](permissions.md)）；`ToolSessionContext` 负责**执行期强制检查**。没有 OS 级隔离，且不做。

## 8. 路径安全

词法规范化只能处理 `.` / `..` 和前缀，**不能证明真实文件仍在授权根目录内**——工作区内的 symlink 可以指向外面；对尚不存在的新文件，只检查目标字符串也无法证明父目录没越界。

所有文件工具必须通过**同一个异步解析入口**获得已检查路径：

```rust
pub enum PathIntent { MustExist, MayCreate }
pub enum PathAccess { Read, Write }

pub struct CheckedPath { /* 字段私有 */ }

async fn resolve_path(&self, input: &str,
                      access: PathAccess, intent: PathIntent)
    -> Result<CheckedPath, ToolExecutionError>;
```

`CheckedPath` **字段保持私有**，只有文件系统 Backend 能消费——防止工具检查完路径后又换回未验证的 `PathBuf`。

解析规则：

1. 以 `working_directory` 解析相对路径；
2. 词法规范化，拒绝明显越界；
3. canonicalize 所有授权根目录；
4. 已存在目标 → canonicalize 目标本身；
5. 新目标 → canonicalize **最近的已存在父目录**；
6. 验证真实目标或真实父目录位于允许的 canonical root 内；
7. 再执行只读 / 写入 / 保护目录判断；
8. 返回 `CheckedPath`，由 Backend 完成实际操作。

第 7 步的保护目录里包含一个用户级 skill 根：`~/.agents/skills/`。它在工作目录之外，必须通过 Core 持有的 `SkillRoots` 显式授权；对 `read` / `grep` / `glob` / `list` **可读**，对 `write` / `edit` **一律拒绝**。该根为 `None` 时不纳入；`.claude/skills/` 不是来源。没有 project、bundled 或 `.openwork/skills/` 根。理由是闭环——能改 skill 就能让一次提示注入变成跨 Session 持久的提权，见 [skills.md §5.2](skills.md)。

对创建路径仍需防止"检查后父目录被替换"。首选方案是 Backend 在已验证父目录下创建临时文件并同目录 rename。

### Shell 重定向边界

`bash` 不能只检查启动目录——`printf x > ../outside.txt`、`tee /abs/path`、`ln -s /outside target` 都会直接写文件。

语法预检可以识别常见的 `>` / `>>` / `tee` / `cp` / `mv` / `ln` 目标，用于提前审批和友好报错，但**覆盖不了变量展开、子 Shell、脚本文件和工具自身的间接写入**。

因此：

- 语法预检**不是安全边界**，它挡不住变量展开、符号链接、子进程自身的写入与网络访问；
- 能真正强制这条边界的只有操作系统级隔离，而本项目**不做**（[permissions.md §1.4](permissions.md)）；
- 因此 `bash` 只在两种情况下自动执行：命令通过**只读判定**（程序 + 参数子集的封闭白名单，[permissions.md §2.3](permissions.md)），或在 `acceptEdits` 下命中**文件系统命令闸门**（[permissions.md §4.8](permissions.md)）。**其余一律逐次确认**，审批卡片就是它的边界（[permissions.md §5](permissions.md)）；
- 上述两条自动放行路径的边界是判定表本身的正确性，因此它们必须在 Trace 中留下可反查的来源（[permissions.md §7](permissions.md)）；
- 任何情况下**文案不得声称已限制写入或网络**。

## 9. 内置工具

七个工具，两组：`filesystem/{read, write, edit, grep, glob, list}` 与 `process/{bash}`。

**Skill 不增加第八个工具。** `SKILL.md` 与 `references/` 走 `read`、`scripts/` 走 `bash`——一个专用的 `skill(name)` 工具能做的事 `read` 已经全能做，而工具定义的常驻成本每次 Model Call 都要付。理由见 [skills.md §4.4](skills.md)。用户从 `$` 候选框显式选择是 Turn 输入，不是 Tool Call。

### read

通过 `CheckedPath` 读取；分配大字符串前先检查文件大小；明确报告过大、二进制和无效 UTF-8。

可选 `offset`（零基行偏移）与 `limit`（按行，最大 2000）。未给 `limit` 时返回 offset 之后全部内容，仍受 1 MiB 全局字节上限约束。

### write

`PathIntent::MayCreate` 验证真实父目录；共享原子写入；单次写入字节上限；返回创建或覆盖状态。**父目录创建必须逐层验证**，不能先递归创建再补权限检查。

`write` 表示"写入完整文件"，不承担局部修改职责。

### edit

精确字符串替换语义：`old_text` 不存在 / 出现多次 / 与新文本相同都失败；空 `old_text` 只用于创建新文件。

并发保护：

1. 每个 canonical path 一把异步写锁；
2. 读取原始内容并计算摘要；
3. 计算替换结果；
4. **提交前再次验证当前摘要**；
5. 文件已变化则返回 stale edit，**不覆盖并发修改**；
6. 共享原子写入提交。

**不把 `edit` 改成补丁语言。** 可靠的 `apply_patch` 需要独立定义多文件、hunk 定位、偏移容忍、部分失败和回滚语义；没有这些契约就把两种编辑方式塞进一个参数会降低可预测性。

### grep

流式目录遍历；每个目录项和读取阶段都检查取消；`glob` 过滤统一匹配**相对工作区路径**；跳过超限文件和二进制并汇总 skipped 数；达到 `maxResults` 立即停止；拒绝 `maxResults = 0`。保持 `content` / `files_with_matches` / `count` 三种模式。单行过长时输出仍受字节上限约束，**截断必须显式标注**。

### glob

流式遍历；**不忽略取消令牌**；统一匹配相对路径；达上限提前结束而不是先收集整棵树；`maxResults` 默认 200、范围 1–2000；输出确定性排序。

`glob` 不等同于 Shell 命令——它跨平台、受权限控制、结果有界且不执行任意代码，因此保留。

### list

安全路径解析后列出单层目录，文件类型是枚举而非布尔：

```rust
pub enum EntryKind { File, Directory, Symlink, Other }
```

零基 `offset` + `limit`，默认每页 200、最大 2000。分页基于排序后的稳定结果。

### bash

**保持前台、单次调用**，不增加后台任务生命周期——只有启动后台任务而没有对应的查询和终止工具，会留下无法管理的进程和不完整的权限闭环。

**有界输出**：stdout/stderr 必须持续 drain（避免子进程因管道写满而阻塞），但内存只保留固定大小的 head 与 tail，最终组合并标注省略字节数。**不能先 `read_to_end` 再截断**——否则 `yes` 这类命令仍会耗尽内存。

**退出与取消**：

- 正常退出：返回 `exit_code`、stdout、stderr、耗时、截断信息；
- **非零退出仍是最终工具结果**，不转换成基础设施错误；
- 启动失败 / 管道失败 / Backend 故障：返回工具执行错误；
- timeout 或取消：终止**整个进程组**，返回终止前已收集的部分输出；
- 结果明确区分 `exited` / `timed_out` / `cancelled`，`spawn_failed` 是独立错误类型。

**网络**：不管控，也不声称（[permissions.md §1.4](permissions.md)）。没有隔离手段就没有可强制的网络边界，因此不设网络相关环境变量，结果里也不附网络注解——一个恒为“未强制”的免责声明只会训练用户忽略它。

## 10. 结果、进度与文件变更

### 结构化结果

模型最终收到文本，但工具内部不构造任意字符串：

```rust
pub struct BashResult {
    pub status: ProcessStatus,
    pub exit_code: Option<i32>,
    pub stdout: CapturedOutput,
    pub stderr: CapturedOutput,
    pub duration_ms: u64,
}
```

模型适配器只序列化 `ToolResult` 的文本 `output`，**不把 Artifact 放进模型上下文或 Token 统计**。

### 进度通道

```rust
pub enum ToolProgress {
    Stdout { chunk: String },
    Stderr { chunk: String },
    Message { message: String },
}
```

边界：Progress 是**临时观察数据，不写入 Conversation**；一次调用仍只产生一个最终 `ToolResult`；**进度发送失败不改变执行结果**；Core 把它转成 `tool_call_progress` Live Update；不折叠进 Snapshot——断线重同步只恢复最终结果，不恢复历史进度。

因此增加进度能力**不需要数据库迁移**。

### 文件变更 Artifact 与 Undo

`write` / `edit` 成功改变文本文件时生成 `kind = "file_change"` Artifact。它与 Progress 生命周期不同：

- Artifact 是最终 `ToolResult` 的一部分，随工具消息写入 `messages.content` JSONB；
- 保存路径、变更 ID、创建/修改类型、准确增删行数、带三行上下文的 diff hunk、前后 SHA-256，以及撤销所需的原内容；
- **桌面端不重新读磁盘去"猜" diff**，因此会话重载后仍显示当时的准确变更；
- Undo 根据会话和变更 ID 从已持久化的 Artifact 恢复，并把对应 Artifact 标记 `undone = true`；
- Undo 与同一会话的新 Turn 串行；**哈希不匹配时返回冲突**，不覆盖用户或外部进程后来写入的内容；
- 同一批次对同一路径的多次变更**按逆序**撤销——"先创建、再编辑"最终会删除该文件。

它同时也是压缩运行状态 `edited_paths` 的唯一来源（见 [compaction.md](compaction.md)）。

**边界：** 这是 `write`/`edit` 的文本文件变更历史，**不是 Git 快照或任意文件系统事务**。为了生成可持久化 diff 和可撤销内容，覆盖目标必须是可读、UTF-8 且不超过 1 MiB 的文本文件。创建文件的"校验内容后删除"在通用文件系统 API 上仍有外部进程抢占的极小竞态。若文件系统已撤销成功而 JSONB 状态写回失败，磁盘与会话标记可能短暂不一致——**不能把这条链路描述成数据库与文件系统的单一原子事务**。

## 11. 验收

### 契约与分层

1. Definitions 与 Dispatch 来自同一个 `FinalizedToolset`；
2. finalize 之后无法增删替换工具；
3. Input Schema 与反序列化类型来自同一个 Rust 类型；
4. 权限过滤后不可见的工具既不广告也不可执行；
5. 输入未变化时工具定义顺序确定；
6. `openwork-tools` 不出现对 `openwork-core` 的依赖。

### 路径安全

7. 指向工作区外的 symlink 被拒绝；
8. 新建文件时，父目录越界被拒绝；
9. 工具无法绕过 `CheckedPath` 直接拿到 `PathBuf`；
10. `Allow` 不能绕过路径边界。

### 取消与进程

11. 取消 Turn 时所有活动 Tool Call 同时取消；
12. timeout 与用户取消走同一条终止路径；
13. bash 终止整个进程组，并返回终止前已收集的输出；
14. kill 确认失败时返回 `outcome_unknown` 且不遗留孤儿进程；
15. `yes` 这类无限输出的命令不会耗尽内存。

### 工具语义

16. `edit` 在 `old_text` 缺失、重复或与新文本相同时失败；
17. 并发修改导致摘要变化时 `edit` 返回 stale 而不覆盖；
18. `grep` / `glob` 达到上限后停止遍历而不是先收集全树；
19. `read` / `list` 的分页在稳定排序上进行；
20. bash 非零退出是正常工具结果，不是基础设施错误。

### 结果与 Artifact

21. 模型上下文中不出现 Artifact，Token 统计不包含它；
22. 进度发送失败不改变工具结果；
23. 会话重载后仍能显示当时的准确 diff；
24. Undo 在哈希不匹配时返回冲突；
25. 同批次多次变更按逆序撤销。
