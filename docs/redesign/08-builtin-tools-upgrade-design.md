# OpenWork 内建工具升级设计

> 状态：阶段 A 已实施，当前代码未提交；阶段 B、C 尚未实施。
>
> 日期：2026-07-18。
>
> 范围：只升级现有 `read`、`write`、`edit`、`grep`、`glob`、`list`、`bash` 七个工具；不新增工具。
>
> 前置设计：[07-tool-runtime-design.md](07-tool-runtime-design.md)。本设计不改变四层工具运行时，只完善工具实现、共享后端与调用结果语义。

## 1. 决策摘要

OpenWork 保留现有七个内建工具及其稳定 ID，不照搬 `grok-build`、Codex 或 OpenCode 的工具数量和命名。升级重点按优先级分为三层：

1. **P0 安全与资源边界**：统一 canonical path 与 symlink 防护、原子写入、有界 Bash 输出、明确进程退出语义和网络隔离边界；
2. **P1 大目录与大文件可用性**：有界读取、流式遍历、及时取消、相对路径匹配、结果上限和二进制文件处理；
3. **P2 可观测性与结果表达**：内部结构化结果、可选进度通道、分页和更明确的文件类型。

第一阶段不修改公开工具 Schema，因此模型调用、桌面端和数据库均不需要迁移。后续若增加可选参数，也必须保持现有调用兼容。

### 1.1 当前实施状态

截至 2026-07-18，阶段 A 已完成：

- 六个文件类工具统一通过 `CheckedPath` 做词法路径、canonical root、symlink 与 dangling symlink 检查；
- `read` 在返回内容前执行 1 MiB 有界读取和 UTF-8 校验；
- `write`、`edit` 共用同目录临时文件、flush、fsync、权限保留和 rename 的原子写入；
- `edit` 使用会话级 canonical path 写锁，并在提交前比较原始内容，拒绝 stale edit；
- Bash stdout/stderr 持续 drain，但每条流只保留 16 KiB head/tail 和总字节数；
- Bash 非零退出作为已完成进程结果返回，timeout 与取消保留部分输出；
- `NetworkMode::Restricted` 仍未获得操作系统级强制隔离，当前结果会明确报告“请求了限制但 Backend 未强制执行”。

仍未实施的是阶段 B 的流式目录遍历和可选分页参数、阶段 C 的 Tool Progress，以及 Bash 的操作系统级文件/网络沙箱。这些缺口不能因为阶段 A 完成而被标记为已具备。

## 2. 为什么需要升级（实施前状态）

当前七个工具已经覆盖基本工作区编码能力：

| 工具 | 当前职责 | 主要缺口 |
|---|---|---|
| `read` | 读取 UTF-8 文件并附加行号 | 先整文件读入内存，缺少分段读取与二进制识别 |
| `write` | 创建父目录并覆盖写入 | 非原子写入，失败时可能留下部分内容 |
| `edit` | 精确替换唯一的旧文本 | 非原子写入，读取与提交之间缺少并发变更保护 |
| `grep` | 正则搜索文件内容 | 遍历先完整收集，取消和结果上限介入较晚 |
| `glob` | 按模式查找路径 | 忽略调用取消，固定上限，匹配对象不是统一的相对路径 |
| `list` | 列出单层目录 | 无分页，只区分目录和非目录，无法表达 symlink 等类型 |
| `bash` | 前台执行 Shell 命令 | 输出在截断前可能无限累积，非零退出被当成工具基础设施错误 |

这些问题不说明需要更多工具。真正缺少的是现有工具的安全边界、资源上限、取消语义和稳定结果契约。

## 3. 设计原则

### 3.1 保持工具表面稳定

- 保留七个工具 ID；
- 第一阶段不修改参数 Schema；
- 不把 `glob`、`write` 或 `edit` 合并进 `bash`；
- 工具定义与执行继续由 `FinalizedToolset` 保证一致；
- 新能力优先进入共享 Backend，而不是在七个工具中各复制一套逻辑。

### 3.2 默认有界

文件大小、目录遍历数量、搜索命中数量、进程运行时间和输出内存都必须有明确上限。截断不能静默发生，最终结果必须携带 `truncated`、已处理数量或总字节数等信息。

### 3.3 区分三类失败

1. **调用无效**：参数错误、路径不存在、替换文本不唯一；
2. **策略拒绝**：路径权限、审批、网络或进程策略不允许；
3. **执行结果**：命令正常启动并以非零状态退出。

第三类不是工具运行时故障。`bash` 返回 `exit_code = 1` 时，模型应当能够读取 stderr 并决定下一步，而不是只收到一个不透明的 `ToolExecutionError`。

### 3.4 不夸大安全保证

环境变量 `OPENWORK_NETWORK_RESTRICTED=1` 只能向子进程传递意图，不能阻止网络访问。只有进程后端真正应用操作系统级隔离后，才能把该模式称为“已强制限制网络”。

## 4. 统一路径安全

### 4.1 当前风险

词法路径规范化只能处理 `.`、`..` 和路径前缀，不能证明真实文件仍在授权根目录内。例如，工作区内的 symlink 可以指向工作区外；对一个尚不存在的新文件，仅检查目标字符串也无法证明其父目录没有越界。

所有文件工具必须通过同一个异步解析入口获得已检查路径，工具实现不再自行 `join` 后直接访问文件系统。

### 4.2 目标接口

```rust
pub enum PathIntent {
    MustExist,
    MayCreate,
}

pub enum PathAccess {
    Read,
    Write,
}

pub struct CheckedPath {
    lexical: PathBuf,
    canonical_anchor: PathBuf,
}

pub async fn resolve_path(
    &self,
    input: &str,
    access: PathAccess,
    intent: PathIntent,
) -> Result<CheckedPath, ToolExecutionError>;
```

`CheckedPath` 的字段保持私有，只有共享文件系统 Backend 能消费它，避免工具检查完路径后又换回未经验证的 `PathBuf`。

### 4.3 解析规则

1. 以调用上下文的 `working_directory` 解析相对路径；
2. 做词法规范化，拒绝明显越界；
3. canonicalize 所有授权根目录；
4. 对已存在目标 canonicalize 目标本身；
5. 对新目标 canonicalize 最近的已存在父目录；
6. 验证真实目标或真实父目录位于允许的 canonical root 内；
7. 再执行只读、写入、保护目录等权限判断；
8. 返回 `CheckedPath`，由 Backend 完成实际操作。

对创建路径仍需防止检查后的父目录被替换。首选方案是由 Backend 在已验证父目录下创建临时文件并完成同目录 rename；如果平台能力允许，再逐步引入基于目录句柄的相对打开，进一步缩小 TOCTOU 窗口。

### 4.4 Shell 重定向边界

`bash` 不能只检查命令启动目录，因为以下命令会直接写文件：

```sh
printf data > ../outside.txt
tee /absolute/path/file
ln -s /outside target
```

Shell 语法预检可以识别常见的 `>`、`>>`、`tee`、`cp`、`mv`、`ln` 等写入目标，用于提前审批和友好报错，但它不能覆盖变量展开、子 Shell、脚本文件或工具自身的间接写入。

因此目标边界是：

- 语法预检属于 defense-in-depth；
- 真正的文件与网络限制必须由进程 Backend 的操作系统沙箱强制执行；
- 在强制沙箱尚未实现前，`bash` 继续保持高风险审批，文案不得声称已完全限制写入或网络。

## 5. 共享文件系统 Backend 升级

七个工具不应分别解决相同的资源和一致性问题。共享 Backend 增加以下能力：

### 5.1 有界读取

读取时先读取元数据并检查上限，再分块解码，而不是 `read_to_string` 后才判断是否过大。返回结果至少表达：

```rust
pub struct TextRead {
    pub content: String,
    pub bytes_read: u64,
    pub truncated: bool,
}
```

无效 UTF-8 与疑似二进制文件应返回明确错误；不能把解码失败伪装成空内容。

### 5.2 原子写入

`write` 与 `edit` 共用原子写入流程：

```text
在目标同目录创建唯一临时文件
  -> 写入全部内容
  -> flush
  -> 按配置执行 fsync
  -> 尽可能保留原文件权限
  -> rename 覆盖目标
  -> 清理失败后的临时文件
```

同目录临时文件保证 rename 不跨文件系统。结果应区分 `created`、`overwritten` 和 `unchanged`，并记录写入字节数。

### 5.3 流式遍历

当前 `walk_files` 先返回完整 `Vec<PathBuf>`，大仓库中会增加启动延迟和内存占用。目标接口应逐项产生结果，例如返回 `BoxStream<Result<WalkEntry, _>>`，让 `grep` 和 `glob` 能够：

- 每处理一项检查取消；
- 达到结果上限后立即停止；
- 不把整个目录树保存在内存；
- 统一使用相对工作区路径进行 glob 匹配；
- 继续尊重 `.gitignore` 和显式排除规则。

## 6. 各工具升级方案

### 6.1 `read`

第一阶段：

- 通过 `CheckedPath` 读取；
- 在分配大字符串前检查文件大小；
- 明确报告文件过大、二进制内容和无效 UTF-8；
- 保留当前行号输出格式。

第二阶段可增加兼容的可选参数：

```json
{
  "path": "src/main.rs",
  "offset": 1,
  "limit": 200
}
```

`offset` 和 `limit` 按行计算，并在结果中说明实际范围及是否还有后续内容。未提供参数时保持当前行为，但仍受全局字节上限约束。

### 6.2 `write`

- 通过 `CheckedPath(PathIntent::MayCreate)` 验证真实父目录；
- 采用共享原子写入；
- 设置单次写入字节上限；
- 返回创建或覆盖状态；
- 父目录创建必须逐层验证，不能先递归创建再补权限检查。

`write` 仍表示“写入完整文件”，不承担局部修改职责。

### 6.3 `edit`

继续保留精确字符串替换语义：

- `old_text` 不存在时失败；
- 出现多次时失败；
- 新旧文本相同时失败；
- 空 `old_text` 只用于创建尚不存在的文件。

在此基础上增加：

1. 每个 canonical path 的异步写锁；
2. 读取原始内容并计算内容摘要；
3. 计算替换结果；
4. 提交前再次验证当前摘要；
5. 文件已变化时返回 stale edit，不覆盖并发修改；
6. 使用共享原子写入提交。

本阶段不把 `edit` 改为补丁语言。可靠的 `apply_patch` 需要独立定义多文件、hunk 定位、偏移容忍、部分失败和回滚语义；在没有这些契约前，把两种编辑方式混在一个参数里会降低可预测性。

### 6.4 `grep`

- 使用流式目录遍历；
- 对每个目录项和文件读取阶段检查取消；
- `glob` 过滤统一匹配相对工作区路径；
- 跳过超过上限的文件和二进制文件，并汇总 skipped 数量；
- 达到 `maxResults` 后立即停止遍历；
- 拒绝 `maxResults = 0`，并设置全局最大值；
- 保持 `content`、`files_with_matches`、`count` 三种模式。

对于单行过长的文件，输出仍受最终结果字节上限约束，截断必须显式标注。

### 6.5 `glob`

- 使用流式目录遍历；
- 不再忽略 `ToolCallContext` 的取消令牌；
- 统一匹配相对工作区路径；
- 达到上限后提前结束，而不是先收集整个目录树；
- 第二阶段增加可选 `maxResults`，默认值保持当前行为并受全局上限限制；
- 输出继续确定性排序；若为了排序必须缓冲，只缓冲已受限的候选结果。

`glob` 不等同于 Shell 命令。它是跨平台、受权限控制、结果有界且不执行任意代码的路径发现工具，因此应保留。

### 6.6 `list`

第一阶段通过安全路径解析列出单层目录，并将文件类型从布尔值扩展为内部枚举：

```rust
pub enum EntryKind {
    File,
    Directory,
    Symlink,
    Other,
}
```

第二阶段增加可选 `offset` 和 `limit`，避免超大目录产生不可控输出。排序规则固定，分页必须基于排序后的稳定结果。

### 6.7 `bash`

`bash` 保持前台、单次调用模型，不增加后台任务生命周期。原因是只有启动后台任务、没有对应的查询和终止工具，会留下无法管理的进程和不完整的权限闭环。

#### 有界输出

stdout 与 stderr 必须持续 drain，避免子进程因管道写满而阻塞；但内存只保存固定大小的 head 与 tail：

```text
持续读取管道
  -> 累计 total_bytes
  -> 保留前 N 字节
  -> 环形缓冲保留后 M 字节
  -> 最终组合，并标注中间省略字节数
```

不能先 `read_to_end` 再截断，否则 `yes` 等命令仍可能耗尽内存。

#### 退出与取消

- 成功启动并退出：返回 `exit_code`、stdout、stderr、持续时间和截断信息；
- 非零退出：仍是最终工具结果，不转换成基础设施错误；
- 启动失败、管道失败或 Backend 故障：返回工具执行错误；
- timeout 或取消：终止整个进程组，并返回状态及终止前已收集的部分输出；
- 最终结果明确区分 `exited`、`timed_out` 和 `cancelled`；`spawn_failed` 作为独立的工具执行错误类型返回。

#### 网络模式

`NetworkMode::Restricted` 只有在 Backend 已应用平台级限制时才表示强制隔离。实施前可以保留该枚举用于策略决策，但结果或日志中必须说明 enforcement 状态，不能只靠环境变量宣称网络已被限制。

## 7. 结果与进度语义

### 7.1 内部结构化结果

当前模型最终仍可接收文本，但工具内部不应只构造任意字符串。建议每个工具先生成结构化结果，再由统一 Formatter 转为模型文本：

```rust
pub struct BashResult {
    pub status: ProcessStatus,
    pub exit_code: Option<i32>,
    pub stdout: CapturedOutput,
    pub stderr: CapturedOutput,
    pub duration_ms: u64,
}
```

这让测试可以断言字段语义，也为以后桌面端展示保留稳定入口。该改动是 Rust 内部实现，不要求数据库增加列。

### 7.2 可选进度通道

第三阶段可以在 `ToolCallContext` 增加可选进度发送器：

```rust
pub enum ToolProgress {
    StdoutChunk(Bytes),
    StderrChunk(Bytes),
    Message(String),
}
```

边界规则：

- Progress 是临时观察数据，不写入 Conversation；
- 一个调用仍只能产生一个最终 `ToolResult`；
- 进度发送失败不得改变工具执行结果；
- `openwork-core` 可把 Progress 转换为 Live Update；
- 前端未实现渲染时可以忽略这些事件。

因此，增加进度能力不需要数据库迁移；只有决定在 UI 中实时展示时才需要前端改动。

## 8. 分阶段实施

### 阶段 A：安全基线，不改公开 Schema（已实施）

1. 引入统一 `CheckedPath` 解析；
2. 为所有文件工具补 canonical path 与 symlink 防护；
3. `write`、`edit` 改用共享原子写入；
4. `edit` 增加并发变更检测；
5. Bash 输出改为有界 head/tail 捕获；
6. 非零退出改为正常的进程结果；
7. 明确 `NetworkMode` 的声明与实际 enforcement 状态；
8. 补齐安全、资源上限和取消测试。

这一阶段不需要修改模型提示、前端、数据库或 SQLx migration。

### 阶段 B：大仓库能力，兼容扩展 Schema

1. Backend 支持流式遍历；
2. `grep`、`glob` 提前停止并及时取消；
3. `read` 增加可选 `offset`、`limit`；
4. `glob` 增加可选 `maxResults`；
5. `list` 增加可选 `offset`、`limit`；
6. 统一二进制、超大文件和截断报告。

新增参数均为可选，旧调用保持有效。Agent 看到的 Schema 会变化，但不涉及持久化结构。

### 阶段 C：可观测性

1. 内部结果类型化；
2. `ToolCallContext` 增加可选 Progress；
3. Core 转发 Tool Progress Live Update；
4. 需要时再增加前端实时输出组件。

## 9. 测试与验收门槛

### 9.1 路径安全

- `../` 词法越界被拒绝；
- 工作区内 symlink 指向外部文件时，读写均被拒绝；
- 新文件的父目录通过 symlink 指向外部时被拒绝；
- 授权根本身包含 symlink 时按 canonical root 正确判断；
- 检查后父目录变化不会导致静默越界；
- 保护路径策略在 canonicalize 后仍生效。

### 9.2 文件一致性

- 原子写入失败不破坏原文件；
- 临时文件与目标位于同一目录并能清理；
- 覆盖时按平台能力保留权限；
- `edit` 检测并发修改并拒绝 stale commit；
- 空替换、多重匹配和 no-op 保持当前失败语义。

### 9.3 资源与取消

- 超大文件在整文件分配前被拒绝；
- 大目录遍历达到结果上限后停止；
- `grep`、`glob` 在遍历中能够及时取消；
- `yes` 或持续输出命令不会导致内存随输出无限增长；
- Bash timeout、取消后整个进程组终止；
- timeout 和取消结果保留已经捕获的有界输出。

### 9.4 结果语义

- `exit 1` 返回 `exit_code = 1`，不是 spawn/backend 错误；
- stderr、截断字节数和持续时间可被确定性断言；
- Progress 不进入 Conversation，最终结果只写入一次；
- 进度消费者断开不影响工具最终结果。

每个阶段至少通过 `cargo test -p openwork-tools`；影响 Session 调用链时，再运行 `cargo test -p openwork-core` 的工具调用、取消和权限相关测试。

## 10. 兼容性与系统影响

| 变更 | 工具 Schema | 前端 | 数据库 | 行为兼容性 |
|---|---|---|---|---|
| 阶段 A 安全与 Bash 修复 | 不变 | 不需要 | 不需要 | 更严格拒绝越界路径；非零退出信息更完整 |
| 阶段 B 可选参数 | 兼容扩展 | 不需要 | 不需要 | 旧参数继续有效 |
| 内部结构化结果 | 不变 | 不需要 | 不需要 | 模型文本格式需保持稳定或显式版本化 |
| Tool Progress | 不变 | 展示时才需要 | 不需要 | 不影响最终 ToolResult |
| 操作系统级 Bash 沙箱 | 不变 | 不需要 | 不需要 | 部分过去可执行的越界副作用会被拒绝 |

本设计没有表结构变更，也不新增 migration。只有未来决定把工具进度或额外结果字段持久化为查询事实时，才需要另行设计存储；当前不实施。

## 11. 明确暂不实施

- 不新增 `apply_patch` 工具；
- 不给 `bash` 增加后台任务、查询和终止生命周期；
- 不删除 `write`、`edit` 或 `glob`；
- 不增加动态 ToolPack、MCP 或运行时动态注册；
- 不拆分 `openwork-workspace` 或 `openwork-tool-runtime` crate；
- 不把 Shell 语法扫描当成安全沙箱；
- 不为 Tool Progress 新建数据库表；
- 不要求前端在阶段 A、B 同步修改。

这些能力不是永久禁止，而是必须在出现明确产品需求、完整生命周期和可测试契约后单独设计。

## 12. 取舍与后果

### 12.1 采用本设计的代价

- 文件 Backend 会比直接调用 `tokio::fs` 更复杂；
- canonical path 与跨平台 symlink 行为需要更多平台测试；
- 原子写入和并发检测增加少量 I/O；
- 流式遍历改变 Backend 接口，需要同步调整 `grep` 与 `glob`；
- 操作系统级 Bash 沙箱需要按 macOS、Linux 分别实现能力探测和降级策略。

这些是实现和长期维护成本，不是功能完成后的额外运行费用。换来的收益是路径权限不再只停留在字符串检查，工具面对大输出和大仓库时具备可预测的资源上限。

### 12.2 未采用的替代方案

| 方案 | 不采用原因 |
|---|---|
| 完整复制 `grok-build` 工具集 | 产品范围、工具生命周期和当前运行时规模不同 |
| 全部文件操作交给 `bash` | 跨平台性、权限审计、结构化结果和可预测性更差 |
| 现在新增 `apply_patch` | 需要先定义独立、可靠的补丁事务契约 |
| 只做 Shell 重定向字符串扫描 | 无法覆盖 Shell 展开和间接副作用，不能形成安全保证 |
| 现在增加后台 Bash | 没有查询和终止工具时生命周期不闭合 |
| 一次性修改七个公开 Schema | 增加模型行为回归面，阻碍先落地 P0 安全修复 |

## 13. 完成定义

只有同时满足以下条件，才能把“现有工具升级”标记为完成：

- 六个文件类工具入口都不能绕过统一真实路径检查；
- `write` 与 `edit` 不会因中途失败留下部分目标文件；
- `edit` 不会静默覆盖读取后的并发修改；
- `grep`、`glob` 在大目录中有界并可取消；
- `read` 不会先无界读取再检查限制；
- Bash 输出内存与最终结果均有明确上限；
- Bash 非零退出、timeout、取消和 Backend 故障语义可区分；
- 网络限制的声明与真实 enforcement 一致；
- 不增加工具数量，不修改数据库，不破坏现有七个工具调用；
- 对应测试覆盖权限绕过、资源耗尽、并发写入和取消路径。
