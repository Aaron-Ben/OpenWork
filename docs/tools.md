# 工具

工具运行时是四层结构，收敛于 `FinalizedToolset`。本文覆盖契约、权限边界、路径安全和七个内置工具的语义。

## 1. 四层

| 层 | 是什么 | 生命周期 |
|---|---|---|
| **工具契约** | 稳定 ID、typed input/output、schema、风险等级、执行逻辑 | 编译期 |
| **工具集选择** | Agent 从已注册工具中选出真正可用的子集 | Agent 构建时 |
| **`ToolSessionContext`** | 工作目录、沙箱后端、文件系统与进程后端、环境 | 与 Session 同寿 |
| **`ToolCallContext`** | 本次调用的 ID、取消令牌、deadline、进度通道、生效的沙箱策略 | 每次调用 |

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
    pub sandbox: Arc<dyn SandboxBackend>,
    pub environment: Arc<HashMap<String, String>>,
    pub filesystem: Arc<dyn AsyncFileSystem>,
    pub process_backend: Arc<dyn ProcessBackend>,
    pub spill: Option<SpillDirectory>,    // ~/.openwork/spill/<session-id>/，§10
    pub observations: FileObservations,   // 先读后改的观察表（§9）；Core 按 Session 持有，跨 Turn 共享
}
```

`SandboxBackend` 来自 `openwork-sandbox`：把一条 argv 包装成受约束的 argv（macOS 上是 Seatbelt），并持有进程级的自检结论——可用或不可用（[permissions.md §3](permissions.md)）。它只包装、不启动进程，启动仍由 `ProcessBackend` 负责。

**它不保存**：沙箱模式、Turn 取消令牌、Tool Call ID、当前 Permission Request、SessionActor、Chat State、ModelPort、数据库连接、Trace Recorder。

沙箱模式不在这一层，因为它在一个 Session 内会变：用户随时切换模式，批准的越界只作用于单次调用。它随调用走（§5）。

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
    pub sandbox_policy: SandboxPolicy,
}
```

`SandboxPolicy` 定义在 `openwork-sandbox`，由 Core 在每次调用前盖章：默认是会话当前的模式；用户批准越界时，这一次的策略带上批准的路径（`path_grants`）（[permissions.md §2.4](permissions.md)）。bash 用它生成 Seatbelt profile，文件工具用它做路径围栏——**两者读的是同一个值**，不会出现同一次调用里 bash 和文件工具各按各的模式判断。

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

Core 决定这次调用**要不要先问**，以及**在哪个沙箱策略下执行**：

```text
硬保护路径 / 越界请求校验失败  → 规则拒绝，不调用工具
越界请求 / 危险命令            → 发出 PermissionRequest，等待用户（只有「允许一次」与「拒绝」）
bash 且沙箱不可用              → 返回 sandbox_unavailable，不执行、不出卡片
其余                            → 以会话模式盖章 sandbox_policy，立即执行
```

判定只看模式、越界请求和危险命令检测，**不看命令文本推断出的效果**（[permissions.md §2.1](permissions.md)）。权限等待会修改 Session 运行时状态、发 Live Update、接收 Desktop 命令，所以它属于 `SessionActor` 的控制流。

### 7.2 执行期强制 —— Tools 与沙箱负责

- **bash**：在 `sandbox_policy` 生成的 Seatbelt profile 下执行，由内核强制；
- **文件工具**：`resolve_path` 按同一个 `sandbox_policy` 做路径围栏（§8）。

> **用户批准能改变这一次调用的沙箱策略，但不能绕过它。** 越界批准换来的是更宽的策略，不是"不检查"；硬保护路径在任何策略下都不可写。

执行期发现的拒绝（内核拒绝某个文件操作、围栏拒绝某个路径）**不是权限判定，是结果事实**，记在 `ToolResult` 上并附拒绝标记（[permissions.md §4.6](permissions.md)）。

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
3. canonicalize 工作区根与临时目录；
4. 已存在目标 → canonicalize 目标本身；
5. 新目标 → canonicalize **最近的已存在父目录**；
6. 按本次调用的 `sandbox_policy` 判断真实目标或真实父目录：读取检查凭据禁读档，写入检查可写根与受保护子路径（[permissions.md §2.3](permissions.md)）；
7. 返回 `CheckedPath`，由 Backend 完成实际操作。

第 6 步的四档路径与 Seatbelt profile **由同一组函数推导**——都来自 `openwork-sandbox`，`openwork-tools` 不自己持有任何路径规则（[permissions.md §2.4](permissions.md)）。读取不再限于工作区：除凭据目录外处处可读，与 bash 在沙箱里能读的范围一致。

用户级 skill 根 `~/.agents/skills/` 属于硬保护档：可读，`write` / `edit` 在任何模式与任何越界下都拒绝。该根为 `None` 时不纳入；`.claude/skills/` 不是来源。没有 project、bundled 或 `.openwork/skills/` 根。理由是闭环——能改 skill 就能让一次提示注入变成跨 Session 持久的提权，见 [skills.md §5.2](skills.md)。

对创建路径仍需防止"检查后父目录被替换"。首选方案是 Backend 在已验证父目录下创建临时文件并同目录 rename。

**这道围栏是策略边界，不是内核边界。** 它的威胁面是模型选定的路径参数，工具代码本身可信，"先规范化再判包含"覆盖得了这个面；解析到系统调用之间残留的 TOCTOU 被就地重新规范化收窄，而不是消除。对不可信代码的隔离由 bash 的沙箱承担。

### bash 的边界是沙箱

`bash` 不能只检查启动目录——`printf x > ../outside.txt`、`tee /abs/path`、`ln -s /outside target`、`cargo build` 里的 `build.rs` 都会直接写文件。从命令文本推断这些写入**不可能完整**：变量展开、子 Shell、脚本文件和程序自身的写入都看不见。

因此：

- bash 的整个进程树在 Seatbelt 沙箱内执行，**写到哪里由内核判断**（[permissions.md §3](permissions.md)）；
- 不做任何基于命令文本的效果推断或自动放行判定。`tree-sitter-bash` 只用于危险命令检测，而那**不是边界**（[permissions.md §4.3](permissions.md)）；
- 沙箱不可用时 bash 不执行，没有"这一次不用沙箱"的选项；**不存在任何不经沙箱启动 bash 的代码路径**（[permissions.md §3.2](permissions.md)）；
- 任何情况下**文案不得声称限制了网络**（[permissions.md §2.5](permissions.md)）。

## 9. 内置工具

七个工具，两组：`filesystem/{read, write, edit, grep, glob, list}` 与 `process/{bash}`。

**Skill 不增加第八个工具。** `SKILL.md` 与 `references/` 走 `read`、`scripts/` 走 `bash`——一个专用的 `skill(name)` 工具能做的事 `read` 已经全能做，而工具定义的常驻成本每次 Model Call 都要付。理由见 [skills.md §4.4](skills.md)。用户从 `$` 候选框显式选择是 Turn 输入，不是 Tool Call。

**每个工具自己保证结果有界。** 工具结果会一直留在 Conversation 里、随每次 Model Call 重发，所以"先全部返回、再由请求投影截断"是最贵的做法：截掉的内容已经丢了，留下的仍然偏大，模型还得再调一次去找被截掉的部分。请求投影里的单条上限只是兜底，不是正常路径。各工具的上限见下文，超出上限的完整内容走 §10 的落盘。

### read

通过 `CheckedPath` 读取；分配大字符串前先检查文件大小；明确报告二进制和无效 UTF-8。

| 参数 | 默认 | 说明 |
|---|---|---|
| `offset` | 1 | **1 基**起始行 |
| `limit` | 2000 | 最多返回的行数 |

三道上限。行数与单行长度与 DSH 一致；字节取 32 KB 而不是 DSH 的 50 KB，与请求投影的单条上限（`openwork-core` 的 `DEFAULT_MAX_TOOL_RESULT_TOKENS` = 8000 token，按 4 字节 / token 即 32 000 字节）对齐——否则 32–50 KB 之间的读取会被投影从中间截断，违背下面"截断只发生在末尾"。**本文的"32 KB"一律指 32 000 字节，且包含结尾的续读 / 落盘提示**，模型看到的整段文本不超过它：


| 上限 | 值 | 超出时 |
|---|---|---|
| 行数 | 2000 | 停在第 2000 行 |
| 字节 | 32 KB | **停在最后一个完整行**，不从中间截 |
| 单行长度 | 2000 字符 | 该行截断并标注 `... (line truncated to 2000 chars)`；超长行只在内存里保留前 8 KB，其余边读边丢，一个几百 MB 的单行压缩文件不会被整行读进内存 |

输出带行号（`{行号}\t{内容}`），供 `edit` 定位。没读完时末尾写明怎么继续：

```text
[showing lines 1-812 of 3401; stopped at 32 KB. Continue with offset=813]
```

**截断只发生在末尾。** 头尾截断适合日志，不适合源码——代码的关键部分往往在中间，从中间截掉等于让模型再读一次。

成功的 `read` 登记一次观察（§9 先读后改）。

### write

`PathIntent::MayCreate` 验证真实父目录；共享原子写入；单次写入字节上限；**父目录创建必须逐层验证**，不能先递归创建再补权限检查。

`write` 表示"写入完整文件"，不承担局部修改职责。**覆盖已存在的文件前必须先读过它**（§9 先读后改）；创建新文件不需要。

返回给模型的是一行摘要：

```text
Created src/new.rs (+42)
Overwrote src/old.rs (+30 -12)
```

### edit

字符串替换：`old_string` 不存在 / 出现多次 / 与新文本相同都失败；空 `old_string` 只用于创建新文件。**目标文件必须先读过**（§9 先读后改）。

**匹配顺序**（借鉴 maka，它的实现又取自 opencode、cline 与 gemini-cli）：

| 顺序 | 策略 | 容忍的差异 |
|---|---|---|
| 1 | `exact` | 无 |
| 2 | `line-trimmed` | 每行首尾空白（缩进不一致） |
| 3 | `whitespace` | 连续空白折叠 |
| 4 | `escape` | 模型把换行、引号等写成转义形式 |

- 每一步都要求**唯一匹配**，找到多处即失败，不往下一步走；
- 非精确策略要求 `old_string` 去空白后至少 5 个字符，且匹配到的范围不得远大于 `old_string`——过短或过宽的模糊匹配最容易改错位置；
- 移植时保留来源许可声明（MIT 与 Apache-2.0），逐段注明出处。

**为什么要容错。** 编辑失败最常见的原因不是改错了意图，而是缩进、空白或转义与文件不一致。每一次失败都会让模型重读整个文件再试一次，那一轮的上下文代价远大于容错匹配本身的风险。

返回给模型的是**带行范围的一行摘要**：

```text
Edited src/storage/time.rs:120-128 (+3 -1)
Edited src/storage/time.rs:120-128 (+3 -1, matched ignoring indentation)
Replaced 4 occurrences in src/lib.rs (+4 -4)
```

- 行范围是替换后新内容所在的行，模型需要确认时可以只读这一段；
- 非精确匹配时注明用了哪种容忍，让模型知道文件里的真实写法与它以为的不同；
- 完整 diff 只进 `file_change` Artifact 给界面看（§10），不进模型上下文。`edit` 是一个 Turn 里调用最频繁的工具，附在它结果上的每一行都会累加。

并发保护：

1. 每个 canonical path 一把异步写锁；
2. 读取原始内容并计算摘要；
3. 计算替换结果；
4. **提交前再次验证当前摘要**；
5. 文件已变化则返回 stale edit，**不覆盖并发修改**；
6. 共享原子写入提交。

**不把 `edit` 改成补丁语言。** 可靠的 `apply_patch` 需要独立定义多文件、hunk 定位、偏移容忍、部分失败和回滚语义；没有这些契约就把两种编辑方式塞进一个参数会降低可预测性。

### 先读后改

借鉴 DSH 的 `fs-observation-policy`。Session 内存里维护一张观察表：

```text
canonical path → 上一次被读到或写入时的内容 SHA-256
```

| 事件 | 观察表 |
|---|---|
| `read` 成功（读全文或其中一段都算） | 记录该文件当前内容的哈希 |
| `write` / `edit` 成功 | 更新为写入后的哈希——连续编辑同一文件不需要重读 |
| `write` / `edit` 目标**已存在但表里没有** | 拒绝：`Read <path> before editing it.` |
| 表里有，但磁盘上的哈希已经不同 | 拒绝：`<path> changed since you last read it (by you via bash, or by the user). Read it again before editing.` |
| 目标不存在（创建新文件） | 不需要观察 |

它挡住两件事：**模型没看过就改**（凭记忆或猜测改文件），以及**覆盖用户或 bash 在这期间做的修改**。后者与 edit 内部的摘要复核不同：那一道只防读取与提交之间的并发，这一道防的是"模型上次看到的版本"与"现在的版本"之间的差异。

观察表不落库。进程重启后恢复的 Session 需要重新读目标文件——这与 DSH 相同，代价是重启后第一次编辑多一次读取。

### write / edit / bash 的越界参数

沙箱可用时，这三个工具的 schema 多出一对参数（[permissions.md §4.1–4.2](permissions.md)）：

| 参数 | 取值 | 含义 |
|---|---|---|
| `sandbox_permissions.paths` | `[{path, access: read\|write, scope: exact\|subtree}]`，1–16 条 | 这一次调用额外需要的具体路径 |
| `justification` | 字符串，必填 | 直接展示给用户的一句话理由 |

越界只有路径这一种形态，没有"更宽的模式"。越界只作用于这一次调用：Core 把批准的路径放进本次 `sandbox_policy.path_grants`，下一次调用回到会话模式。

文件工具在敏感路径、工作区外写入时返回与 bash 相同的拒绝标记，走同一条越界流程——通常就是目标文件的一条 `exact` 写授权。硬保护路径返回"任何模式下都不可写"的规则拒绝，越界参数对它无效。

**沙箱不可用时这对参数不出现在 schema 里**——公布一个系统兑现不了的选项，只会产生注定失败的请求。

### grep

用 ripgrep 的库 crate（`grep-searcher` / `grep-regex`）配合已在用的 `ignore` 做遍历，不自己实现匹配，也不随包分发 `rg` 二进制。每个文件与每个匹配都检查取消；跳过二进制文件。保持 `content` / `files_with_matches` / `count` 三种模式，上限固定，不接受 `maxResults` 之类的参数。

**两种路径，各有用途**：`glob` 过滤与 ripgrep 一样**相对搜索根**（`path: "desktop"` 下 `src/**/*.ts` 指 `desktop/src/…`）；结果里显示的路径**相对工作区**，可以直接交给 `read` / `edit`。

`content` 模式的输出按文件分组，三道上限：

| 上限 | 值 | 来源 |
|---|---|---|
| 返回的匹配行总数 | 250 | DSH |
| 每个文件最多 | 50 行 | maka——一个大文件不能占满整份结果 |
| 单行长度 | 2000 字节，超出截断并标注 | DSH |

`files_with_matches` / `count` 两种模式最多列 250 个文件，同样报告准确总数并落盘完整列表。

**报告准确总数。** 达到上限后**继续扫描、只计数不保留**，结尾写明：

```text
[showing 250 of 1834 matching lines in 97 files; full list saved to <path>]
```

准确总数让模型知道该不该收窄搜索；"可能还有更多"给不了它这个判断依据。完整列表按 §10 落盘。

扫描受 30 秒超时约束。超时时报告已扫描部分的计数，并写明"至少"：`[at least 1834 matching lines; search timed out after 30s — narrow the path or glob]`。内存只保留返回的那 250 行与计数，与总匹配数无关。

### glob

与 grep 共用遍历；**不忽略取消令牌**；匹配与显示的路径规则与 grep 相同。

- 返回上限 **100** 条（DSH）；
- **按修改时间倒序**：最近改过的文件几乎总是这次任务相关的那些。遍历时用一个容量 100 的堆保留最新的条目，内存与匹配总数无关；
- 结尾报告准确总数，超出上限时完整列表按 §10 落盘；
- 与 grep 相同的 30 秒超时。

`glob` 不等同于 Shell 命令——它跨平台、受权限控制、结果有界且不执行任意代码，因此保留。

### list

安全路径解析后列出单层目录，文件类型是枚举而非布尔：

```rust
pub enum EntryKind { File, Directory, Symlink, Other }
```

零基 `offset` + `limit`，默认每页 200、最大 2000。分页基于排序后的稳定结果。

### bash

**在沙箱内执行**：`sandbox-exec -p <profile> -- bash -c <command>`，profile 由本次调用的 `sandbox_policy` 生成（[permissions.md §3.1](permissions.md)）。执行 shell 是 `bash`，与危险命令检测解析所用的语法一致。

**环境**：继承 OpenWork 进程的环境，再叠加 `SandboxEnvironment::bash_environment()` 给出的变量，覆盖同名变量。目前只有 `GOCACHE`，指向临时目录下 OpenWork 私有的缓存（[permissions.md §3.1](permissions.md) 工具链缓存）。

**参数：**

| 参数 | 默认 | 说明 |
|---|---|---|
| `command` | 必填 | |
| `description` | 必填 | 5–10 个词的动作描述，**只给界面显示**（DSH）。卡片与工具列表上显示"运行测试"比显示一长串命令更容易看懂 |
| `workdir` | 工作区根 | 本次命令的工作目录，相对路径以工作区根解析。每次调用都是新 shell，**不保留 `cd`** |
| `timeoutMs` | 120 000 | 上限 600 000（10 分钟）。大项目的 `cargo build` 常常超过 2 分钟，2 分钟的上限会让它直接失败 |

**有界输出，偏向尾部**：stdout/stderr 必须持续 drain（避免子进程因管道写满而阻塞），内存只保留固定大小的片段。**不能先 `read_to_end` 再截断**——否则 `yes` 这类命令仍会耗尽内存。

模型看到的是：

| 部分 | 大小 |
|---|---|
| 开头 | 2 KB——命令最开始的报错有时就在这里 |
| 结尾 | 14 KB——编译错误、测试失败汇总、最终状态都在这里 |

中间省略时标注省略的字节数，**完整输出按 §10 落盘**并附路径。头尾不对半分：构建输出的开头多半是 `Compiling ...` 这类噪音，真正有用的在结尾。

**前台、单次调用。** 后台任务排在沙箱之后（§11）：后台进程同样必须在沙箱里运行，而它的查询与终止工具要在沙箱就位后一起设计，否则会留下无法管理的进程和不完整的权限闭环。

**退出与取消**：

- 正常退出：返回 `exit_code`、stdout、stderr、耗时、截断信息；
- **非零退出仍是最终工具结果**，不转换成基础设施错误；
- 启动失败 / 管道失败 / Backend 故障：返回工具执行错误；
- timeout 或取消：终止**整个进程组**，返回终止前已收集的部分输出；
- 结果明确区分 `exited` / `timed_out` / `cancelled`，`spawn_failed` 是独立错误类型；
- 非零退出且 stderr 带 Seatbelt 拒绝特征时，结果标记 `denied`，并在输出末尾追加拒绝标记与越界提示（[permissions.md §3.3、§4.6](permissions.md)）；
- `sandbox-exec` 自身启动失败归为沙箱不可用，不归为 `denied`。

**网络**：不管控，也不声称（[permissions.md §2.5](permissions.md)）。不设网络相关环境变量，结果里也不附网络注解——一个恒为“未强制”的免责声明只会训练用户忽略它。

## 10. 结果、进度与文件变更

### 结构化结果

模型最终收到文本，但工具内部不构造任意字符串：

```rust
pub struct BashResult {
    pub status: ProcessStatus,
    pub exit_code: Option<i32>,
    pub output: CapturedOutput,        // stdout 与 stderr 按到达顺序合并，头 2 KB + 尾 14 KB
    pub duration_ms: u64,
    pub sandbox: SandboxFacts,
}

pub struct SandboxFacts {
    pub mode: SandboxMode,         // 执行了就一定在沙箱内
    pub denied: bool,
}
```

**stdout 与 stderr 合并为一路**，与终端里看到的顺序一致：编译错误走 stderr、测试结果走 stdout，分开截断会拆散二者的先后关系，而且两路各留 16 KB 就超出了 bash 的输出上限。进度通道仍区分两路，供界面着色。

模型适配器只序列化 `ToolResult` 的文本 `output`，**不把 Artifact 放进模型上下文或 Token 统计**。

### 大结果落盘

借鉴 DSH 的 spill-policy。任何工具结果的文本超过 **32 KB**（与请求投影的单条上限对齐，理由同 read），或某个工具达到了自己的上限（grep 与 glob 的条数、bash 的截断），**完整内容写进落盘文件**，模型只看到有界的部分加上一句怎么取回：

```text
... (41 318 bytes omitted. Full output saved at ~/.openwork/spill/<session-id>/<tool-call-id>.txt — use read with offset/limit, or grep, to look at it.)
```

- **位置**：`~/.openwork/spill/<session-id>/`。它属于硬保护档（[permissions.md §2.3](permissions.md)），模型可读、不可写，因此落盘内容不会被沙箱里的命令篡改；由 OpenWork 进程自己写入；
- **生命周期**：随 Session 删除而删除；启动时清理超过 7 天的目录；
- **落盘失败不影响结果**：写不进去就只返回有界部分，并去掉"saved at"这一句，不能给出一个读不到的路径；
- **read 不落盘**：它读的文件本身就是完整内容，续读提示里的 `offset` 已经是取回方式，再复制一份只是浪费磁盘；
- **单个落盘文件不超过 64 MB**：`yes` 这类无限输出在超时前能写出数 GB，写满即停，结果里注明"只保存了前 64 MB"；
- **内存有界**：grep / glob 的完整列表先在内存里缓冲（256 KB），超出后转为流式写文件；bash 在输出超过头尾容量时才建文件。

它解决的是"截断即丢失"：有界部分控制上下文成本，完整内容仍然可以按需取回，模型不必为了看被截掉的那段而重新执行命令。

### 旧结果修剪

每个结果都有界，但它们会一直累积在 Conversation 里。压缩触发时，先修剪、再决定要不要摘要，见 [compaction.md §1.1](compaction.md)。

### 错误文本必须可操作

工具失败的文本要告诉模型**下一步该做什么**，而不只是出了什么错：

| 不行 | 可以 |
|---|---|
| `oldString not found` | `old_string not found in src/a.rs. Read the file again and copy the exact text, including indentation.` |
| `doom loop detected for tool 'grep'` | `You have called grep with identical arguments 3 times and got the same result. Look at the previous result, or change the pattern or path.` |
| `failed to read x: No such file` | `src/x.rs does not exist. Use glob to find the file.` |

模型拿不到可操作的出路时，最常见的反应是换个写法重试同一件事，直到耗尽 `max_model_calls`。

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

## 11. 分期

| 阶段 | 内容 | 目的 |
|---|---|---|
| **T0** | read 的三道上限与只在末尾截断；grep / glob 的上限、准确总数、glob 按修改时间排序；bash 偏向尾部的输出；write / edit 的一行摘要；大结果落盘 | 直接压住上下文占用。与沙箱改造互不依赖，可以并行 |
| **T1** | 先读后改；edit 的容错匹配；旧结果修剪（[compaction.md §1.1](compaction.md)）；bash 的 `workdir` / `description` 与 10 分钟超时 | 减少失败的编辑与白跑的回合 |
| **T2** | 可操作的错误文本；重复调用在硬性停止（`doom_loop_threshold`）之前先给一次提醒 | 让模型自己走出死循环 |
| **T3（沙箱之后）** | 后台任务：`bash` 增加 `run_in_background`，返回任务 ID；`job_output` 读取输出（可只读新增部分）；`job_kill` 终止。后台进程同样在沙箱内运行，结果与越界规则与前台相同 | 能跑 dev server 与长任务 |

T3 排在 [permissions.md](permissions.md) P1 之后：后台进程必须在沙箱里运行，它的越界、危险命令检测与沙箱不可用时的停用都要与前台共用同一套，先有沙箱才谈得上一致。

**暂不做：** PTY 与交互式输入（`write_stdin`）；按需加载工具定义（工具只有七个，收益不存在）；`apply_patch`（见 §9 edit）；把文件工具放进独立的沙箱进程（见 [permissions.md §8](permissions.md)）。

## 12. 验收

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
10. 越界批准只改变这一次调用的 `sandbox_policy`，不能解开硬保护路径；
10a. 同一组路径在文件工具围栏与 Seatbelt profile 下得到相同的可读 / 可写结论（对等测试）；
10b. 凭据目录对 `read` / `grep` / `glob` / `list` 与 bash 同样不可读。

### 沙箱

10c. bash 的进程树在 Seatbelt 沙箱内执行，profile 来自本次调用的 `sandbox_policy`；
10d. 沙箱自检失败时 bash 不执行，返回 `sandbox_unavailable`；代码中不存在不经 Seatbelt 启动 bash 的路径；
10e. 被内核拒绝的调用在结果上标记 `denied` 并附拒绝标记；`sandbox-exec` 启动失败不标 `denied`；
10f. 沙箱不可用时 schema 中不出现 `sandbox_permissions` / `justification`。

### 取消与进程

11. 取消 Turn 时所有活动 Tool Call 同时取消；
12. timeout 与用户取消走同一条终止路径；
13. bash 终止整个进程组，并返回终止前已收集的输出；
14. kill 确认失败时返回 `outcome_unknown` 且不遗留孤儿进程；
15. `yes` 这类无限输出的命令不会耗尽内存。

### 工具语义

16. `edit` 在 `old_string` 缺失、在同一匹配策略下出现多次、或与新文本相同时失败；
17. 并发修改导致摘要变化时 `edit` 返回 stale 而不覆盖；
18. `read` / `list` 的分页在稳定排序上进行；
19. bash 非零退出是正常工具结果，不是基础设施错误。

### 有界结果

20. `read` 不带参数读一个 5000 行的文件，返回第 1–2000 行（或到 32 KB 为止的最后一个完整行），末尾写明 `Continue with offset=N`；**不存在从中间截断的输出**；
21. 一个 10 000 字符的单行被截断到 2000 字符并标注；
22. `grep` 返回不超过 250 行、单个文件不超过 50 行、单行不超过 2000 字节，结尾报告准确的匹配行数与文件数；内存占用与总匹配数无关；
23. `grep` / `glob` 超过 30 秒时返回已扫描部分并写明"至少"；
24. `glob` 返回不超过 100 条，按修改时间倒序，报告准确总数；
25. bash 输出超过 16 KB 时模型看到开头 2 KB 与结尾 14 KB，中间标注省略字节数；
26. 达到上限或超过 32 KB 的结果完整写入 `~/.openwork/spill/<session-id>/`，模型看到的文本附带该路径，且能用 `read` 读到它；沙箱内的 bash 不能写这个目录；
27. 落盘失败时结果仍返回，且不出现指向不存在文件的路径。

### 编辑

28. `write` / `edit` 返回一行摘要：`Edited <path>:<起>-<止> (+a -d)`，行范围是新内容所在的行；完整 diff 只出现在 Artifact；
29. 缩进不一致、空白不一致、转义不一致的 `old_string` 在唯一匹配时成功，摘要注明使用的容忍方式；任一策略下出现多处匹配即失败；
30. 去空白后少于 5 个字符的 `old_string` 不做非精确匹配；
31. 没读过的已存在文件，`edit` / `write` 被拒绝并提示先读；
32. 读过之后被 bash 或用户改过的文件，`edit` / `write` 被拒绝并提示重读；
33. 连续两次 `edit` 同一文件，第二次不需要重读；
34. 创建新文件不需要先读。

### bash 参数

35. `workdir` 相对工作区根解析，必须通过沙箱策略；
36. `timeoutMs` 上限 600 000；
37. `description` 显示在界面上，不进入模型可见的结果。

### 结果与 Artifact

38. 模型上下文中不出现 Artifact，Token 统计不包含它；
39. 进度发送失败不改变工具结果；
40. 会话重载后仍能显示当时的准确 diff；
41. Undo 在哈希不匹配时返回冲突；
42. 同批次多次变更按逆序撤销；
43. 工具失败文本都说明下一步该做什么（§10）。
