# 权限系统 P1 实施 brief

> 这是一份**交接文档**，不是功能文档。P1 落地后可以删除。
>
> 规格的唯一事实来源是 [permissions.md](permissions.md)；本文只回答"从当前代码出发，第一期怎么做、怎么测、怎么算完"。两者冲突时以 permissions.md 为准。

## 0. 一句话

把权限判断的基本单位从**工具风险等级**换成**效果**，并把 `bash` 的命令串拆开——这一期不新增任何自动放行，只把地基换掉。

## 1. 本期边界

### 做

对应 [permissions.md §9.1](permissions.md) 的 P1：

1. **效果模型** —— `Read(path)` / `Write(path)` / `Exec(program, args)`（§2.1）
2. **效果提取**
   - 文件工具 → 直接产出效果
   - `bash` → `tree-sitter-bash` 解析、节点白名单遍历（§4.2）、子命令拆分（§4.3）、重定向目标产出独立 `Write` 效果（§4.5）
3. **规则语言与匹配器** —— 效果表达式 + 三判定值（§3.1 §3.2）
4. **合并求值** —— `deny > ask > allow`，与规则顺序无关（§3.3）
5. **内置规则三档**（§3.4）—— 两条硬 deny（不出卡片）、敏感路径 ask、`allow read(<ws>/**)`、`allow write(<ws>/**)`（仅 `acceptEdits`）
6. **两个模式** `default` / `acceptEdits`，**只做文件侧**（§3.5）
7. **审批卡片** —— 逐条呈现、原文始终可见、判定来源标注（§5.1 §5.3）

### 不做（属于后续阶段，不要提前实现）

| 能力 | 阶段 |
|---|---|
| 只读判定器、资格检查 | P2 |
| arity 归约、会话授权、卡片中间按钮 | P3 |
| bash 文件系统命令闸门、重定向 create、`sed` 专项 | P4 |
| `permissions.toml` 加载 | P5 |

**因此 P1 期间：**

- 任何 `bash` 调用都会弹卡片（包括 `ls`）——这是预期行为，不是缺陷；
- 卡片上**只有 `[允许一次]` 和 `[拒绝]` 两个按钮**，没有中间按钮；
- 规则集只有内置那一档，用户规则要等 P5。

**不要因为"反正 `ls` 是安全的"就提前塞一个只读白名单。** 少了 P2 的资格检查，任何 exec 自动放行都是提权路径（§9.1 的分期约束）。

## 2. 起点盘点

当前 `crates/openwork-tools/src/policy/` 是**另一套设计的残留**（Codex 风格的 sandbox profile）。以下几处要替换或删除，**不要在它们之上扩展**。

### 删

| 位置 | 东西 | 依据 |
|---|---|---|
| `policy/profile.rs` | `FileSystemMode::DangerFullAccess`、`PermissionProfile::danger_full_access()` | §1.4 不做逃生门 |
| `policy/profile.rs`、`backend/mod.rs:77`、`backend/process.rs` | `NetworkMode` 整个枚举与 `ProcessRequest.network_mode` | §1.4 不做网络管控，也**不做任何声称** |
| `builtins/process/bash.rs:59-66` | 注入 `OPENWORK_NETWORK_MODE` 环境变量 | 同上 |
| `builtins/process/bash.rs:99-105` | 输出里追加 `[network restriction requested but not enforced by this backend]` | **直接违反验收 74**。§1.4 原话："一个恒为'未强制'的免责声明只会训练用户忽略它" |
| `policy/evaluator.rs` | `evaluate(mode, risk_hint)` 与 `PermissionMode { NeverAsk, Ask }` | 判断单位错了，见下 |

`registry.rs:244`、`builtins/mod.rs:39`、`builtins/process/bash.rs:146` 里的 `danger_full_access()` 是测试 fixture，一并换成工作区 profile。

### 换

**`evaluate(mode, risk_hint)` 必须整体废弃。** 它的输入是 `ToolRisk`（`ReadOnly` / `WorkspaceMutation` / `ProcessExecution`），而 `ToolRisk` 表达不了效果模型——`bash("cat x")` 和 `read("x")` 在它眼里是两个类别，而验收 1 要求它们命中同一条规则。

`ToolRisk` 本身**保留**，它在 `registry.rs` 里还用于工具集选择，那是另一件事。

**`PermissionProfile` 的角色要反过来。** 现在它是手写的 roots + protected_names；按 §3.4「内置规则同时驱动执行期强制」，它应当**由内置规则集派生**，而不是和规则并存两套。P1 保留 `PermissionProfile` 作为执行期强制的视图，但构造函数改成从内置规则生成。

### 留

**路径解析链已经基本对了，不要重写。** `context.rs` 的 `resolve_path` → `resolve_creatable_path` → `check_canonical_path` 已经做到：

- 相对路径按 `working_directory` 解析、词法归一化先做快速拒绝；
- `MustExist` canonicalize 目标本身，`MayCreate` canonicalize 最近的已存在父目录；
- 悬空 symlink 显式拒绝；
- 授权根目录本身也 canonicalize 后再比较。

这条链符合 [tools.md §8](tools.md)。P1 要改的只有 `check_canonical_path` 里那句 `DangerFullAccess` 早退，以及让 roots / protected 来自内置规则。

`CheckedPath` 字段私有的约束**必须保持**（验收：工具无法绕过它拿到裸 `PathBuf`）。

## 3. 要建什么

建议在 `crates/openwork-tools/src/permission/` 下新建，逐步把 `policy/` 收拢进来（P1 结束时 `policy/` 可以只剩 `filesystem.rs` 的路径工具）。

```
permission/
  mod.rs
  effect.rs      Effect、EffectSet、效果的展示形态
  rule.rs        Rule、RuleBehavior、EffectPattern、匹配器
  builtin.rs     内置三档规则 + 由它派生 PermissionProfile
  engine.rs      合并求值 + 决策流水线（§4.1）
  mode.rs        PermissionMode { Default, AcceptEdits }
  card.rs        审批卡片的结构化载荷
  bash/
    mod.rs
    parse.rs     tree-sitter-bash + 节点白名单
    split.rs     子命令拆分
    effects.rs   子命令 → EffectSet（含重定向）
```

### 核心类型骨架

只给形状，细节自己定。

```rust
// effect.rs
pub enum Effect {
    Read(PathBuf),
    Write(PathBuf),
    Exec { program: String, args: Vec<String> },
}

// rule.rs
pub enum RuleBehavior { Allow, Ask, Deny }

pub enum EffectPattern {
    Read(PathGlob),
    Write(PathGlob),
    Exec(ExecPattern),          // TokenPrefix(Vec<String>) | Literal(String)
}

pub enum RuleScope { Builtin, Global, Workspace, Session }

pub struct Rule {
    pub id: RuleId,
    pub pattern: EffectPattern,
    pub behavior: RuleBehavior,
    pub scope: RuleScope,
}

// engine.rs
pub enum AskSource {
    ExplicitRule,       // 用户写的 ask（P5 才有来源，枚举先留着）
    BuiltinSensitive,   // 内置敏感路径
    NoRuleCovers,       // §3.4 的默认
}

pub enum Verdict {
    Allow { source: DecisionSource, rule: Option<RuleId> },
    Ask   { source: AskSource,      rule: Option<RuleId> },
    Deny  { rule: RuleId, silent: bool },   // silent = 内置两条，不出卡片
}
```

**`Deny.silent` 不是可选的。** 内置的 `.git` / `.openwork` 写入拒绝**不产生卡片**（§3.4、验收 6），必须在类型上区分，不能靠调用点记得判断。

### 卡片载荷

`PermissionRequest`（`crates/openwork-core/src/session/commands.rs:39`）现在只带 `tool_name` + `input: Value` + `reason: String` 一个字符串。P1 要把它换成结构化载荷，否则 §5.1 的"逐条呈现"在前端做不出来。

```rust
pub struct ApprovalCard {
    pub units: Vec<CardUnit>,   // 逐条，已放行的也要在列表里（验收 49）
    pub raw: String,            // 原文，始终可见（验收 48）
}

pub struct CardUnit {
    pub display: String,               // 该子命令原样
    pub effects: Vec<EffectDisplay>,
    pub verdict: UnitVerdict,          // 含 AskSource，供 §5.3 文案分档
    pub outside_workspace: bool,       // 验收 53
}

pub enum EffectDisplay {
    Inferred(Effect),                       // "写 build/"
    TrustedProgram { program: String },     // "执行（信任该程序）"
    // P2 会加 ReadonlyProven —— 现在不要提前加
}
```

`PermissionDecision { Allow, Deny }` P1 保持不变，会话授权是 P3 的事。

## 4. 实现顺序

六步，每步都有一个可独立验证的完成信号。**按顺序做**，不要并行开工。

| # | 内容 | 完成信号 |
|---|---|---|
| 1 | **清场**：删 §2「删」表里的全部内容，测试 fixture 换成工作区 profile | `grep -rn "NetworkMode\|DangerFullAccess\|OPENWORK_NETWORK_MODE" crates/` 零命中；`cargo test` 绿 |
| 2 | **效果模型 + 规则匹配器**（纯函数，无 IO） | 表驱动测试覆盖 glob 语义（`*` 不跨目录 / `**` 跨目录）、exec token 前缀、`exec(cargo test)` 不匹配 `cargo testsuite`（验收 25） |
| 3 | **内置规则三档 + 合并求值** | 验收 2、5、6、7 通过；`PermissionProfile` 改为由内置规则派生，`resolve_path` 行为不回归 |
| 4 | **bash 解析**：tree-sitter-bash + 节点白名单 + 子命令拆分 + 重定向 | 验收 22、24、26 通过；`echo $(whoami)` 与引号不闭合走**同一条**返回路径 |
| 5 | **决策流水线接入** `FinalizedToolset::authorize`（`run_loop.rs:548`） | 验收 1、3、10、12、13 通过 |
| 6 | **卡片载荷 + 桌面端渲染** | 验收 48、49、52、53、56、57、58 通过 |

第 1 步单独拆出来，是因为那些残留会持续误导后面每一步的判断。

### 依赖

需要新增 `tree-sitter` + `tree-sitter-bash` 到 workspace 依赖。**不要自研 tokenizer**（§4.2：引号、转义、`$()`、heredoc，自研每漏一种形态就是一条绕过）。

## 5. 怎么测

### 分层

| 层 | 怎么测 | 占比 |
|---|---|---|
| 效果提取、规则匹配、合并求值、bash 解析 | **纯函数表驱动**，不碰文件系统 | 应占绝大多数 |
| 路径解析与执行期强制 | `tempdir` + 真实 symlink | 少量但必须有 |
| 决策流水线 | in-memory fixture：`(模式, 规则集, 调用) → 期望 Verdict` | 中等 |
| 卡片 | **结构化断言**，不要对渲染后的字符串整段比对 | 少量 |

### 命名约定

测试名带上 [permissions.md §9.2](permissions.md) 的验收编号，便于反查覆盖面：

```rust
#[test]
fn acc_01_bash_cat_and_read_hit_same_rule() { … }

#[test]
fn acc_06_builtin_git_deny_produces_no_card() { … }
```

### 必须有的 fixture

1. **工作区内指向工作区外的 symlink** —— 验收：写入被拒；
2. **不存在的深层新建路径**，其某级父目录是越界 symlink —— 验收：`MayCreate` 被拒；
3. **`.git/config`、`.openwork/permissions.toml`、`.env`、`.vscode/settings.json`** 四个路径在 `default` / `acceptEdits` 下各自的期望结果（前两个 silent deny，后两个 ask）；
4. **工作区根目录本身** —— `write(<ws>)` 必须落不进 `write(<ws>/**)`（§3.4）。

### 表驱动的形状

规则匹配和 bash 解析都适合这样写，一行一个 case，加 case 的成本要足够低：

```rust
#[test]
fn acc_22_compound_commands_split_into_units() {
    let cases = [
        ("rm -rf build && cargo test", 2),
        ("a | b | c",                  3),
        ("cat x; echo y",              2),
        ("echo hi",                    1),
    ];
    for (input, expected_units) in cases {
        assert_eq!(split_units(input).unwrap().len(), expected_units, "input: {input}");
    }
}
```

## 6. 完成的定义

全部勾上才算 P1 完成：

- [ ] `cargo test --workspace` 全绿；`cargo clippy --workspace --all-targets` 无 warning；`cargo fmt --check` 通过
- [ ] `grep -rn "NetworkMode\|DangerFullAccess\|OPENWORK_NETWORK_MODE" crates/ apps/` **零命中**
- [ ] 工具结果、UI 文案、日志中**不出现任何网络限制或沙箱隔离的表述**（验收 74）——这条要人工过一遍，grep 查不全
- [ ] `evaluate(mode, risk_hint)` 已删除，权限决策不再读 `ToolRisk`
- [ ] 有一条测试守住 **P1 不变量：任何 `Exec` 效果都不会得到 `Verdict::Allow`**（无论模式、无论命令）——这条防的是有人"顺手"加只读白名单
- [ ] 内置两条 deny 走 `silent: true`，不产生 `PermissionRequest`
- [ ] 卡片上只有两个按钮，代码里不存在中间按钮的分支
- [ ] `CheckedPath` 字段仍私有，工具拿不到裸 `PathBuf`
- [ ] PR 描述里列出：**本期覆盖了 §9.2 的哪些编号、哪些编号因依赖 P2–P5 而暂缓（附原因）**

最后一条不要省——那份清单是 P2 的输入。

### 建议的验收编号覆盖面

至少这些应该在 P1 内变绿：**1, 2, 3, 5, 6, 7, 10, 12, 13, 22, 24, 26, 48, 49, 52, 53, 56, 57, 58, 74**。

部分可达、需要在 PR 里说明的：4（"显式 ask 规则"那一档要等 P5）、8（后半"用户手写 deny 升级"要等 P5）、9（需要用户规则）、27（后半要等 P2 的只读判定）。

## 7. 四个最容易做错的地方

这几处在 spec 里是**反直觉的收紧**，看起来像 bug 或缺失，实际是刻意的。改动它们之前先回去读对应小节。

**① 内置两条 deny 不出卡片。**
看起来像漏了 UI 分支。实际理由（§3.4）：`.openwork` 存放权限配置本身，一次被说服的批准就是一条完整的自我提权链——**可被批准的边界不是边界**。

**② `write(<工作区>/**)` 不覆盖工作区根本身。**
看起来像 glob 写错了。实际是为了让 `rm -rf .` 即使在 `acceptEdits` 下也要问（§3.4）。不要"顺手修正"成包含根目录。

**③ "拆不动"和"解析失败"必须走同一条代码路径。**
看起来可以分开报错给更好的提示。实际理由（§4.2 §4.7）：`echo $(whoami)` 语法完全合法、解析完全成功，但藏着一条看不见效果的子命令。用户看不出区别，代码里多一档就多一处会走偏的分支。

**④ 节点白名单，不是黑名单。**
遇到白名单之外的具名节点立即判定"拆不动"。进程替换 `<(...)`、算术展开 `$((...))`、大括号展开会自动落到拒绝一侧，不需要事先想到它们（§4.2）。

## 8. 交付物

1. 代码 + 测试（按 §5 的分层与命名）
2. PR 描述含验收编号覆盖清单（§6 最后一条）
3. 如果实现过程中发现 permissions.md 里有**做不到或自相矛盾**的条款，不要自行绕过——在 PR 里单独列出来，那是文档的问题

**动手前先给实现方案**，尤其是模块划分与 `PermissionProfile` 派生方式，方案确认后再写代码。
