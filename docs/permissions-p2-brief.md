# 权限系统 P2 实施 brief

> 交接文档，P2 落地后可删除。P1 的 brief（`permissions-p1-brief.md`）此时可以一并删掉。
>
> 规格的唯一事实来源是 [permissions.md](permissions.md)。两者冲突时以它为准。

## 0. 一句话

第一次打开 `exec` 的自动放行 —— 因此这一期的重点不是"让 `ls` 不再弹窗"，而是**让这道门只对能逐条论证的命令打开**。

## 1. 本期边界

### 做

对应 [permissions.md §9.1](permissions.md) 的 P2：

1. **资格检查**（§2.4 / §4.4）—— 五道闸，只关自动放行这一侧的门
2. **只读判定器**（§2.3）—— 程序白名单 + 每程序的标志白名单（**带 arity**）+ 不可静态求值 token 的拒绝
3. **只读判定产出效果集**，效果照常过 §4.1 的规则流水线
4. **Trace 字段**：`readonlyProofKey`、`permissionRuleId`、`permissionRuleScope`（见 §6）

### 不做

| 能力 | 阶段 |
|---|---|
| arity 归约、会话授权、卡片中间按钮、模式切换按钮 | P3 |
| bash 文件系统命令闸门、重定向 create、`sed` 专项 | P4 |
| `permissions.toml` 加载、用户手写规则 | P5 |

**P2 结束后的预期行为：** `ls src` / `cat README.md` / `rg foo src` / `git status` / `git diff` 在 `default` 下直接执行；`cargo test` / `npm install` / `git commit` 仍然逐次弹卡片；卡片上仍然只有两个按钮。

### 为什么这一期最危险

P1 无论怎么写都不会自动执行 `exec` —— 引擎里有一道硬降级。**P2 是这道门第一次打开。**

> **资格检查与只读判定必须在同一个 PR 里交付，不接受拆分。**

少了资格检查，只读判定器本身就是提权路径：模型往工作区写一个叫 `ls` 的脚本，然后请求 `./ls` 或 `PATH=. ls` —— 判定器看到程序名 `ls`，认为它是 coreutils 的那个，直接放行。这不是理论风险，是这套设计里最短的一条自我提权链。

## 2. 起点：P1 留下了什么

### 可以直接用的

| 东西 | 位置 |
|---|---|
| `InvocationAnalysis` / `AnalysisUnit`（已拆好的子命令 + 效果） | `permission/effect.rs` |
| tree-sitter-bash 解析、节点白名单、`unparsed` 标志 | `permission/bash/mod.rs` |
| 规则合并求值（`deny > ask > allow`，顺序无关） | `permission/engine.rs::authorize_effect` |
| `ExecutionPermit`（批准的效果集带到执行期） | `permission/effect.rs` |
| `PermissionProfile::from_builtin_rules` | `permission/builtin.rs` |

### 两处必须处理的 P1 遗留

**① `engine.rs::authorize_effect` 里的 exec 降级守卫 —— 保留，不要删。**

```rust
if matches!(effect, Effect::Exec { .. }) && rule.behavior == RuleBehavior::Allow {
    return UnitVerdict::Ask { source: AskSource::NoRuleCovers, rule_id: None };
}
```

它只管**规则来源**的 exec allow。P2 的只读判定**不是规则**，走并列的内置证据路径（§4.1：规则层在前，内置证据在后）。而 P2 期间根本不存在规则来源的 exec allow——用户规则是 P5，会话授权是 P3。

所以这道守卫在 P2 保持原样，等 P3 引入会话授权时再按 §4.6.1 的归约放开。**看到它不要"顺手清理"。**

**② `acc_12_p1_never_auto_allows_exec` 改名，断言不变。**

改成 `acc_12_no_mode_auto_allows_unprovable_exec`。它用的是 `cargo test`，而 `cargo` 不在只读判定表里，所以断言（两种模式都 Ask）在 P2 后仍然成立——但名字里的 "p1" 会误导。改名后它正好是验收 12 的原话："不存在任何自动放行任意 exec 的模式取值；`cargo test` 在两种模式下都要问"。

## 3. 要建什么

```
permission/
  eligibility.rs     五道闸（§4.4）
  readonly/
    mod.rs           判定入口 + 效果产出
    table.rs         程序白名单 + (标志, arity) 表
```

### 只读判定表的形状

**标志表的条目必须是 `(标志, arity)` 二元组，不能是标志集合。** 这不是风格问题：

```
git diff -S -- --output=/tmp/pwned
```

若判定表把 `-S` 记成不吃参数：判定器前进一格 → 看到 `--` → 认为后面全是路径、停止检查标志。
而 git 认为 `-S` 必须吃一个参数 → 吞掉 `--` → 光标落在 `--output=...` → 按长选项解析 → **任意文件写入**。

判定器和真实程序对同一串 argv 理解不同，叫 **parser differential**。它是这张表最主要的失效模式，不是边角情况。

```rust
enum Arity { None, One }          // 这个标志吃不吃下一个 token

struct ReadOnlyCommand {
    /// "ls" 或 "git log" —— 多 token 键优先于单 token 键匹配
    key: &'static str,
    flags: &'static [(&'static str, Arity)],
    /// 位置参数怎么解释：全部是路径 / 第一个是模式其余是路径 / 都不是路径
    operands: OperandKind,
}
```

起步程序集见 [permissions.md §2.3①](permissions.md)。**可以先做一个子集**，但每加一条都要能逐条论证；未登记的程序和未登记的标志一律不可证明。

### 不可静态求值的 token（§2.3④）

判定前先扫一遍所有 token，命中任一即不可证明：

- 含 `$`（变量展开、命令替换）
- 同时含 `{` 与（`,` 或 `..`）（大括号展开）
- 含反引号
- `rg` / `grep` 的模式参数含换行或回车

第一条最关键：`git diff "$Z--output=/tmp/pwned"` 在判定器眼里是一个不以 `-` 开头的位置参数，在 bash 眼里是一个长选项。

### 效果产出

**只读判定证明的是"效果集合就是这些 read"，不是"这次调用被批准"。**

```
判定通过 → effects = 每个显式路径操作数的 Read(...)
                   + 一条隐式 Read(<工作区>)
```

产出的效果**照常过 §4.1 的规则流水线**。于是自动得到：

| 命令 | 结果 | 原因 |
|---|---|---|
| `git status` | 自动执行 | 无显式操作数，隐式 `Read(<ws>)` 命中内置 allow |
| `cat src/main.rs` | 自动执行 | `Read(<ws>/src/main.rs)` 命中内置 allow |
| `cat /etc/passwd` | **问** | 判定通过，但 `Read(/etc/passwd)` 在工作区外，无规则覆盖 |
| `cat .env` | 自动执行 | 内置敏感档只拦 `write`，§3.4："保护目标是不可篡改，不是保密" |

**不要把"判定通过"直接翻译成 `Allow`。** 那会丢掉上表第三行，也是 P2 最容易写错的一处。

### 资格检查五道闸（§4.4）

在子命令拆分之后、查规则与进只读判定**之前**执行。

| 闸 | 怎么落地 |
|---|---|
| ① 前置环境变量赋值 | 语法树上 `command` 节点带 `variable_assignment` 子节点。**不区分变量名**，`RUST_LOG=debug` 也不合资格 |
| ② 主语在解释器/wrapper 清单 | `sh` `bash` `zsh` `python*` `node` `ruby` `perl` `ssh` `xargs` `env` `timeout` `nice` `stdbuf` `npm run` …。按 basename 匹配，`/usr/bin/env` 同样命中 |
| ③ 逃逸标志 | §4.4 那张 (程序, 标志) 表。**它服务于用户手写的 allow 规则**（P5），只读判定靠自己的标志白名单封闭，不依赖它 |
| ④ 程序名解析自工作目录 | 见 §7，这条需要你先定一个口径 |
| ⑤ 脚本内 cwd 变更 | `cd` / `pushd` / `env -C` 出现即整条脚本不合资格 |

**资格检查只关自动放行一侧的门。** `deny` 和 `ask` 照常匹配（验收 37）。

## 4. 实现顺序

| # | 内容 | 完成信号 |
|---|---|---|
| 1 | 资格检查五道闸（④ 按 §7 的口径）+ 对抗性测试 | 验收 32–37 通过。此时还没有任何 exec 自动放行，所以这一步**不改变任何行为**——它是先把门装好 |
| 2 | 只读判定表 + 判定器（先做 `ls` `cat` `pwd` `rg` `git status` 五条，跑通链路） | 验收 14 的子集通过 |
| 3 | 效果产出 + 接进 `authorize` 的内置证据阶段 | 验收 15 通过（`cat /etc/passwd` 仍要问） |
| 4 | 补齐判定表其余程序与标志（含 git 子命令） | 验收 14、19、20 通过 |
| 5 | Trace 三个字段 | 验收 21、72 通过 |

第 1 步排在最前面且单独可验证，是刻意的：**先装门，再开门。** 反过来做，中间那段时间里判定器是裸奔的。

## 5. 怎么测

### 对抗性测试是这一期的主体

普通的"`ls` 能自动执行"只能证明功能在，证明不了边界在。这一期必须有一组**明确针对绕过的测试**，直接照抄 [permissions.md §9.2](permissions.md) 的 16–21 与 30–37：

```rust
#[test]
fn acc_16_flag_arity_mismatch_cannot_smuggle_a_long_option() {
    // -S 登记为吃一个参数 → -- 被 -S 吞掉 → --output= 落到未登记标志
    assert_not_provable("git diff -S -- --output=/tmp/pwned");
}

#[test]
fn acc_17_escape_flags_are_not_on_the_program_flag_whitelist() {
    assert_not_provable("rg . --pre=bash FILE");
}

#[test]
fn acc_18_tokens_containing_dollar_are_never_provable() {
    assert_not_provable(r#"git diff "$Z--output=/tmp/x""#);
}

#[test]
fn acc_36_workspace_resident_executables_never_match() {
    // 模型写下 ./ls 后
    assert_not_provable("./ls");
    assert_not_provable("PATH=. ls");
}
```

**每加一个程序到判定表，就要问一遍"它有没有能变成解释器的标志"**，并为答案写一条测试。`rg --pre`、`git -c core.pager`、`find -exec`、`base64 -o` 是已知的四类，不要假设列全了。

### 表驱动

判定表本身适合一张大表：

```rust
let cases = [
    ("ls src",                    Provable),
    ("ls --color=always src",     Provable),
    ("ls --some-unknown-flag",    NotProvable),   // 未登记标志
    ("git log --oneline -n 5",    Provable),
    ("git log --ext-diff",        NotProvable),   // 逃逸标志不在白名单
    ("git reflog expire",         NotProvable),   // 写子命令
    ("cargo test",                NotProvable),   // 程序不在表里
];
```

### 命名

继续用 `acc_<编号>_<描述>`。P1 已覆盖的 19 个编号见那份 PR 描述，不要重复占用。

## 6. Trace 字段

三个字段一起加：`readonlyProofKey`、`permissionRuleId`、`permissionRuleScope`。

**不需要数据库迁移。** `trace_spans.attributes` 是 `JSONB` 列（`202607260001_initial_schema.sql:317`），`ToolTraceAttributesV1` 里加 `Option<String>` 字段是纯追加。两件事要注意：

- 该结构体带 `deny_unknown_fields`，**加字段前先写一条 round-trip 测试**：确认缺字段的旧 JSON 仍能反序列化成 `None`；
- 决定是否要 bump `schema_version`。倾向于不 bump——纯可选追加不破坏旧读者。

`run_loop.rs` 里 P1 留的 `TODO(P2)` 与 `let _ = rule_id;` 在这一期消掉。

`readonlyProofKey` 记的是命中的判定表条目（`"git log"` 而不是完整命令）。理由见 §7：**判定表是我们自己背书的东西，写错时唯一的补救是按条目反查历史上哪些调用因它自动执行了。**

`permissionDecisionSource` 增加取值 `readonly_proof`。

## 7. 需要你先定一个口径：闸 ④ 怎么判"程序名解析自工作目录"

这是本期唯一一个 brief 决定不了的地方。

`permission_analysis` 目前是**同步**函数（`tool.rs:45`），而 `session.filesystem` 是异步的——所以闸 ④ **做不了异步的 PATH 查找**。三个选项：

| 方案 | 代价 |
|---|---|
| **A. 纯词法（推荐）** | 程序名含 `/` → 按工作目录解析，落在工作区内即不合资格。裸名字：若 `PATH` 里**任何一项**落在工作区内，则所有裸名字一律不合资格 |
| B. 把 `permission_analysis` 改成 async | 改动面大（trait、adapter、registry、run_loop），但能真正解析 PATH |
| C. 会话启动时预扫工作区里的可执行文件，缓存一份集合 | 有 TOCTOU 窗口：模型在会话中途写下 `./ls` 就绕过了 |

**推荐 A。** 它完全同步、严格保守，代价只是"把 `PATH` 指进工作区"这种本来就很奇怪的配置下会多问几次。C 有真实的绕过窗口，直接排除；B 的改动量不该由 P2 承担。

如果确认 A，闸 ④ 就是纯字符串判断，`permission_analysis` 签名不动。**动手前请确认这一条。**

## 8. 最容易做错的四处

**① 把"判定通过"当成"批准"。**
判定只产出效果，效果还要过规则。`cat /etc/passwd` 必须仍然问（验收 15）。

**② 标志表只记名字不记 arity。**
见 §3 的 `git diff -S` 案例。这是本期唯一一个"写得像对的、跑起来也像对的、但能被一行命令打穿"的地方。

**③ 用黑名单补逃逸标志。**
只读判定的封闭性来自**程序白名单 + 标志白名单**。§4.4 那张逃逸标志表是给 P5 的用户 allow 规则用的，不是判定器的防线。如果发现"要往逃逸表里加一条才能挡住某个命令"，说明标志白名单开太大了，该收的是白名单。

**④ 删掉 P1 的 exec 降级守卫。**
见 §2①。它管的是规则来源的 exec allow，与只读判定并列，P2 不该动它。

## 9. 交付物

1. 代码 + 测试（对抗性测试是主体，见 §5）
2. PR 描述里列出：本期覆盖了 §9.2 的哪些编号、哪些暂缓（附原因），以及**判定表当前收录了哪些程序**——后者是 P3 之后持续增长的东西，要有个起点记录
3. 实现中若发现 permissions.md 有做不到或自相矛盾的条款，在 PR 里单独列出，不要自行绕过

**动手前先确认 §7 的口径，并给实现方案。**
