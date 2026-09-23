# 开发计划：沙箱权限与工具改造

**这是一份执行计划，不是功能文档。** 设计的唯一权威是 [permissions.md](../permissions.md)、[tools.md](../tools.md)、[compaction.md](../compaction.md) 与 [architecture.md](../architecture.md)；本文只回答**按什么顺序做、做到什么程度算完成、什么时候停下来等决策**。全部工作包完成后删除本文。

## 1. 范围

| 轨道 | 内容 | 设计出处 |
|---|---|---|
| **A 沙箱与权限** | 新建 `openwork-sandbox`；bash 进 Seatbelt；`auto` / `accept-edits` 两个模式；四档路径；路径越界；危险命令检测；删除按命令文本判定的旧代码 | permissions.md P1（§9.1）、architecture.md §2 |
| **B 工具** | 结果有界与落盘（T0）；先读后改、容错编辑、旧结果修剪、bash 参数（T1）；可操作错误与重复提醒（T2） | tools.md §11 T0–T2、compaction.md §1.1 |
| **C 后台任务** | `run_in_background` / `job_output` / `job_kill` | tools.md §11 T3，**A 完成后才开始** |

**不在范围内：** Linux 沙箱（permissions.md P2）、按数据决定的事项（P3）、网络管控、审查模型、会话级授权、PTY——这些都有独立的决策时点，见 §5。

## 2. 顺序

```text
WP0 基线
  └─ WP1  B/T0 结果有界与落盘         ← 小、独立、立刻降低上下文占用
       └─ WP1b B/T1 的一部分：旧结果修剪 + 先读后改（D3 决定提前）
            └─ WP2  A1 openwork-sandbox crate
                 └─ WP3  A2–A5 切换（tools + core + agent + desktop，一次合入）
                      └─ WP4  B/T1 其余：edit 容错匹配、bash 参数
                           └─ WP5  B/T2
                           └─ WP6  C 后台任务
                                └─ WP7  收尾（文档、README、删除本文）
```

**为什么 WP1b 也在沙箱之前（D3）：** WP1 后回放显示模型实际看到的 token 只降了 4%——大结果本来就被请求投影截断，T0 的收益主要是正确性（不再从中间截、准确总数、可取回）。压住**累积**上下文的是旧结果修剪，它只动 Core 的压缩路径，与沙箱无冲突；先读后改减少白改白读，只动 write / edit。二者提前，bash 参数与 edit 容错匹配仍在沙箱之后（它们改 bash 与 edit 的同一片代码）。

**为什么 T0 在沙箱之前：** 两者都改 `bash` 工具与 `openwork-tools`，串行避免冲突；T0 改动面小、验证快，而且直接回应"小任务占用大量上下文"的问题。这个顺序本身是一个决策点（§5 D1）。

**WP3 必须一次合入。** permissions.md §9.1：只上沙箱不上越界，`git commit` 与新增依赖就做不成；只删旧判定不上沙箱，bash 就失去边界。WP3 的内部步骤可以分多次提交，但合入主干是原子的。

## 3. 工作包

每个工作包结束时交付一份**完成报告**（§6），然后才进入下一个。

### WP0 基线

| 内容 | 完成条件 |
|---|---|
| 在独立分支上工作 | 分支已建立 |
| 跑一遍全部检查（§4.1），记录现有失败 | 基线结果写入完成报告；**若已有失败，停下等 D2** |
| 参照任务用**确定性回放**（D2 已定）：对基线提交 `0b44333` 的冻结快照按固定顺序执行一组工具调用（窄 / 宽 grep、glob、读三个大文件、edit、write、三条大输出 bash），记录每次结果的模型可见字节、估算 token（4 字节 / token），以及经请求投影单条上限（8000 token）后的 token | 数字写入完成报告，作为 WP1、WP4 的对照 |

### WP1 工具 T0：结果有界与落盘

| 内容 | 位置 |
|---|---|
| `read`：1 基 `offset`、默认 2000 行 / 32 KB / 单行 2000 字符，只在末尾截断并提示续读 | `openwork-tools/src/builtins/filesystem/read.rs` |
| `grep` / `glob`：改用 `grep-searcher` / `grep-regex`；250 行 / 每文件 50 行 / 单行 2000 字节；准确总数；30 秒超时；glob 100 条按 mtime 倒序（容量 100 的堆） | `grep.rs`、`glob.rs` |
| `bash`：模型可见输出为开头 2 KB + 结尾 14 KB | `process/bash.rs`、`backend/process.rs` |
| `write` / `edit`：一行摘要（含行范围与增删行数） | `write.rs`、`edit.rs` |
| 大结果落盘：`~/.openwork/spill/<session-id>/`，超过 32 KB 或触及工具上限时写入；随 Session 删除；启动时清理 7 天前的目录；落盘失败不给路径 | 新模块；Core 负责清理 |

**完成条件：**

1. tools.md §12 第 20–28 条、第 38–42 条有对应的自动化测试并通过（第 26 条"沙箱内的 bash 不能写落盘目录"在 WP3 验证）；
2. 参照任务回放重跑，与 WP0 对比写入报告。**若没有下降，停下等 D3**；
3. §4.1 全部检查通过。

### WP1b 旧结果修剪 + 先读后改

| 内容 | 完成条件 |
|---|---|
| 先读后改：Session 级观察表（Core 按 Session 持有，跨 Turn 有效） | tools.md §12 第 31–34 条 |
| 旧结果修剪 + `tool_result_pruned_through_sequence` 迁移 | compaction.md §9 "修剪"下全部条目 |

另：参照任务回放重跑；修剪另用一段合成的长会话验证"修剪后低于阈值则不调摘要模型"。§4.1 全部检查通过。

### WP2 沙箱 A1：`openwork-sandbox` crate

| 内容 | 完成条件 |
|---|---|
| `SandboxMode`（`AcceptEdits` / `Auto`）、`SandboxPolicy`、`PathGrant`；四档路径推导函数，带 `actor`（Bash / FileTool） | 单元测试覆盖每一档、每个模式、每种 actor |
| Seatbelt profile 生成：路径经 `-D` 传入；嵌入 `regex` 的路径统一转义 | 单元测试：带引号、括号、空格、正则元字符、非 ASCII 的路径不能改变 profile 结构 |
| 启动自检与结论缓存 | 集成测试：正常时可用；把 `sandbox-exec` 路径指向不存在的文件时报告不可用及原因 |
| 拒绝识别：命令被拒 vs 沙箱本身没启动 | 单元测试 + 集成测试 |
| **真机矩阵**（`#[cfg(target_os = "macos")]`，真实调用 `sandbox-exec`） | 见下表，全部与预期一致；**任何一项与预期不符，停下等 D4** |

真机矩阵（`auto` 模式，除非注明）：

| 场景 | 预期 |
|---|---|
| 依赖已缓存的 `cargo build` / `cargo test` | 成功 |
| `git status` / `git log` / `git diff` | 成功 |
| `git add` / `git commit` / `git reset --hard` / `git stash` | 被拒，工作区不变 |
| 以 `<工作区>/.git` 写越界后的 `git commit` | 成功；`.git/hooks` 仍不可写 |
| `git clean -fd` | 成功（危险命令检测负责拦它） |
| 写 `$HOME`、写 `~/.openwork`、写 skill 根 | 被拒 |
| 读 `~/.ssh`、`~/.aws`、`~/Library/Cookies` | 被拒 |
| 写 `/private/tmp`、写 `$TMPDIR` | 成功 |
| `accept-edits`：`cargo build` 写 `target/`、`touch src/x` | 被拒；以对应路径越界后成功 |
| **常用工具链探测**：`npm install`（依赖已缓存）、`pnpm install`、`pytest`、`go build`、`go test` | **记录实际结果**，交给 D4 决策——它们可能需要写 `~/Library/Caches` 下的缓存目录 |

**对等测试（permissions.md §2.4）：** 对一组覆盖四档路径的路径，比较推导函数的结论与真实内核行为；`accept-edits` 下"工作区对 bash 不可写、对文件工具可写"是唯一显式期望的差异。

**完成条件：** 以上全部通过；crate 不依赖任何其他 OpenWork crate（`cargo tree -p openwork-sandbox` 验证）；§4.1 全部检查通过。

### WP3 沙箱 A2–A5：切换

| 步骤 | 内容 | 位置 |
|---|---|---|
| A2 tools | bash 经 `openwork-sandbox` 以 `bash -c` 启动；拒绝标记与越界提示；`sandbox_unavailable`；`sandbox_permissions` / `justification` 仅在沙箱可用时进 schema；文件工具围栏改用同一组推导函数（读取除凭据目录外处处允许）；硬保护规则拒绝；`ToolCallContext.sandbox_policy` 取代 `ExecutionPermit`；危险命令检测（tree-sitter，只看程序名与标志，包装器剥壳，`bash -c` 递归） | `openwork-tools` |
| A2 删除 | permissions.md §10 列出的全部旧判定代码与 `tests/permissions_p1–p4.rs` | `openwork-tools/src/permission/` |
| A3 core | 会话模式落库（迁移，遵守 `.claude/rules/database.md`）；每次调用盖章策略；越界校验（permissions.md §4.2）；两种卡片的 `PermissionRequest`；危险命令只在 `auto` 单独出卡；非交互 Session 的拒绝文本；子 Agent 模式 = min(父会话, 角色上限)，派生时快照；`runtime/sandbox-policy` world state section（含 bash 是否可用）；Trace 新属性；删除会话规则与会话授权 | `openwork-core` |
| A4 agent | `AgentDefinition.sandbox_ceiling`；explorer 为 `AcceptEdits`；更新 explorer 系统提示词的 bash 边界说明 | `openwork-agent` |
| A5 desktop | 模式指示器（`auto` / `accept-edits`，一键切换）；越界卡片（逐条列路径与档位）；危险命令卡片（高亮命中段）；"沙箱不可用，bash 已停用"常驻提示；Trace 时间线的新类别 | `desktop/` |

**完成条件：**

1. permissions.md §9.2 **第 1–50 条**逐条有证据：自动化测试名，或无法自动化时的手动验证记录（截图 / 步骤 / 结果）；
2. tools.md §12 第 7–10f 条、multi-agent.md §11 第 15–16 条通过；
3. **代码中不存在不经沙箱启动 bash 的路径**：`rg -n 'Command::new\("(sh|bash)"\)' crates` 只命中 `openwork-sandbox` 的包装入口（或为空）；
4. **旧判定代码已删除**：`crates/openwork-tools/src/permission/` 下只剩危险命令检测；`rg -n 'PermissionMode::Default|readonly_proof|allow_eligible|reduce_exec_grant|ExecutionPermit|session_rules' crates desktop/src` 无结果；
5. Desktop 真实运行（`pnpm tauri dev`）走通 §4.2 的手动场景；
6. §4.1 全部检查通过。

### WP4 工具 T1

| 内容 | 完成条件 |
|---|---|
| edit 容错匹配（line-trimmed / whitespace / escape），保留来源许可声明 | 第 29–30 条；许可声明逐段注明 |
| bash `workdir` / `description` / 10 分钟超时 | tools.md §12 第 35–37 条 |

另：参照任务重跑，与 WP0、WP1 的数字一起写入报告。

### WP5 工具 T2

| 内容 | 完成条件 |
|---|---|
| 可操作的错误文本 | tools.md §12 第 43 条；逐个工具列出改前 / 改后文本 |
| 重复调用在硬性停止之前先提醒一次 | 测试：同参数第 N 次调用得到提醒，第 `doom_loop_threshold` 次仍按现有规则停止 |

### WP6 后台任务（C）

**开始前先经 D8 确认设计细节**（tools.md §11 只给了形态）。完成条件在 D8 时补入本节。

### WP7 收尾

1. 删除 permissions.md §10"尚未实施"；更新 compaction.md §8 与 tools.md 中已落地的描述；
2. 根目录 `README.md` 与 `README.en.md` 的"安全边界"表按新设计重写（工作台 bash 一行目前写的是"不提供执行期隔离"）；
3. 更新 [desktop.md](../desktop.md) 中权限卡片与模式的描述；
4. 删除本文。

## 4. 通用完成条件

### 4.1 每个工作包都要过的检查

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
cargo test --workspace
```

```bash
cd desktop && pnpm typecheck && pnpm test
```

- **`cargo test` 必须在设置了 `TEST_DATABASE_URL` 的环境下运行。** 现有 postgres 测试在它缺失时会静默返回，不设置就等于没跑；
- clippy 不新增警告；
- macOS 真机沙箱测试不得标 `#[ignore]`——它们是这次改造的核心证据。

### 4.2 WP3 的手动场景（Desktop 真实运行）

| # | 操作 | 预期 |
|---|---|---|
| 1 | 新建会话，让模型读文件、改文件、跑 `cargo test` | 不出卡片；指示器显示 `auto` |
| 2 | 让模型 `git commit` | 被拒后出越界卡片，列出 `<工作区>/.git`；允许一次后成功；再提交一次再次出卡 |
| 3 | 让模型删除 `src/` 下一个目录 | `auto` 下出危险命令卡片；拒绝后 Turn 停止 |
| 4 | 切到 `accept-edits`，让模型改文件并跑 `cargo build` | 改文件不出卡；`cargo build` 写 `target/` 被拒后出越界卡片 |
| 5 | 让模型读 `~/.ssh/config` | 被拒；模型收到可操作说明 |
| 6 | 让模型写 `.git/hooks/pre-commit` | 规则拒绝，不出卡片 |
| 7 | 派 explorer 子 Agent 调查问题 | 子 Agent 能跑 `git log` / `rg`；写入被拒；不出任何卡片 |
| 8 | 模拟沙箱不可用（测试开关让自检失败），重启 | 常驻提示出现；bash 返回 `sandbox_unavailable`；文件工具照常 |
| 9 | 重启应用 | 会话模式保持为重启前的值 |
| 10 | 打开 Trace | 能区分沙箱内执行、被拒、用户批准的越界、危险命令、规则拒绝 |

### 4.3 什么不算完成

- 某条验收"应该没问题"但没有测试或手动记录；
- 测试在未设置 `TEST_DATABASE_URL` 的环境下"通过"；
- 为了让测试通过而修改了设计文档里的数字、清单或行为，却没有经过 §5 的决策；
- 旧代码留着"以防万一"，或加了兼容开关（AGENTS.md：不保留兼容路径）。

## 5. 决策点

三类事项，处理方式不同：

| 类别 | 处理 |
|---|---|
| **必须你决策** | 我停下，写清楚现状、选项和我的建议，等你回复后才继续 |
| **通知你** | 我按设计文档执行，在完成报告里说明 |
| **我自行决定** | 设计文档没规定、且不改变行为边界的实现细节，不单独汇报 |

### 5.1 必须你决策

| # | 时点 | 需要决定什么 |
|---|---|---|
| **D1** | 开始前 | §2 的顺序（T0 先于沙箱）是否同意；是否在独立 worktree 上工作 |
| **D2** | WP0 基线发现已有失败时 | 先修、跳过，还是作为已知问题带着走 |
| **D3** | WP1 后参照任务的 token 没有明显下降时 | 是否调整上限数字，或继续推进 |
| **D4** | WP2 真机矩阵与预期不符，或常用工具链（npm / pnpm / pytest / go）日常构建需要写工作区外的缓存目录时 | 是否把这些缓存目录（如 `~/Library/Caches/go-build`、pnpm store）默认加入可写根；这会修改 permissions.md §2.3，原计划留给 P3 |
| **D5** | 实现中发现设计文档与现实冲突、有歧义或做不到时（例如 Seatbelt 表达不了某条规则） | 怎么改设计。**先改文档，再改代码** |
| **D6** | 任何对封闭清单或数字的修改 | 硬保护 / 敏感 / 凭据禁读清单、危险命令清单、越界校验规则、工具上限（行数、字节、条数、超时、落盘阈值、修剪阈值）。实现时觉得某个数字不合适，只提建议，不改 |
| **D7** | WP3 的 Desktop 界面 | 模式指示器的位置与文案、两种卡片和"沙箱不可用"提示的样式。我先给截图，你确认后再定稿 |
| **D8** | WP6 开始前 | 后台任务的细节：输出保留多少、任务数上限、会话结束时是否终止、`job_output` 是否只返回增量 |
| **D9** | 每次提交、推送、合入主干前 | 是否提交 / 合入。**我不自行提交或推送** |

### 5.2 通知你

- 数据库迁移的具体 DDL（会话模式列、修剪水位线列），按 `.claude/rules/database.md` 编写，已有会话的模式取 `auto`；
- 从 maka 移植 edit 容错匹配时的许可声明（MIT 与 Apache-2.0）；
- 新增依赖（`grep-searcher`、`grep-regex` 等）；
- 模型可见文本（工具描述、拒绝标记、越界提示、错误文本）的具体措辞——以设计文档的示例为准；
- 发现与本计划无关的缺陷——记录下来，不顺手修。

### 5.3 以后再决策（不在本计划内）

| 事项 | 触发条件 |
|---|---|
| 工具链缓存是否默认可写、是否需要会话级越界授权（permissions.md P3） | 功能上线后，Trace 中 `escalationPaths` 积累了足够数据 |
| Linux 沙箱（permissions.md P2） | 需要支持 Linux 时 |
| 网络管控、是否改为读取默认拒绝 | 你决定接入网络管控时 |

## 6. 完成报告格式

每个工作包结束时提交一份：

```text
工作包：WPn
状态：完成 / 阻塞（阻塞时写明等哪个决策）

验收对照：
  permissions.md §9.2 #12  → sandbox::probe::tests::self_check_detects_missing_runner
  permissions.md §9.2 #27  → 手动：步骤 … 结果 …
  …（每条都要有）

检查结果：fmt / clippy / test（含 TEST_DATABASE_URL）/ desktop typecheck + test
参照任务：回放的逐项字节、token 与投影后 token（WP0 / WP1 / WP4）
与设计的偏差：无 / 列出并注明对应决策号
通知事项：§5.2 中本次涉及的条目
下一步：WPn+1
```
