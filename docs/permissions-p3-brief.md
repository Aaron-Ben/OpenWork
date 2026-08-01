# 权限系统 P3 实施 brief

> 交接文档，P3 落地后可删除。`permissions-p1-brief.md` 与 `permissions-p2-brief.md` 此时可以一并删掉。
>
> 规格的唯一事实来源是 [permissions.md](permissions.md)。两者冲突时以它为准。

## 0. 一句话

把"不被打断"做完 —— **重复的批准在一次会话内不再问**（会话授权），**本来就不该问的不再问**（判定表扩充）。

## 1. 本期边界

### 做

对应 [permissions.md §9.1](permissions.md) 的 P3，外加 P2 留下的判定表尾巴：

**主线一：会话授权与卡片按钮**

1. **arity 归约字典**（§4.6.1）
2. **会话授权** —— `exec` 类，只活在 `SessionActor` 内存
3. **卡片模式切换**（§4.6.2）—— `write` 类不给路径范围授权
4. **按钮选择逻辑**（§4.6.3）—— 只提供能整体解开这次调用的最小会话变更
5. `PermissionDecision` 扩取值 + 桌面端三按钮

**主线二：判定表扩充**

6. 把只读判定表从 6 条扩到覆盖日常高频（§3.6）
7. 补齐资格检查的解释器/wrapper 清单（§3.7）

两条主线并进的理由：它们是同一个用户问题的两半。只做主线一，用户仍会觉得"怎么还是老弹" —— 而那不是会话授权的锅，是 `head` / `wc` / `git log` 还不可证明。

### 不做

| 能力 | 阶段 |
|---|---|
| bash 文件系统命令闸门、重定向 create、`sed` 专项 | P4 |
| `permissions.toml` 加载、用户手写规则 | P5 |
| 规则管理界面、会话授权导出为规则 | P6 |

## 2. 起点：P2 留下什么

### 可以直接用的

| 东西 | 位置 | P3 怎么用 |
|---|---|---|
| `ExecPattern::TokenPrefix(Vec<String>)` | `permission/rule.rs` | 归约结果直接就是它，不需要新类型 |
| `RuleScope::Session` | 同上 | 会话授权的 scope，枚举已经预留 |
| `Rule` + 合并求值（`deny > ask > allow`，顺序无关） | `permission/engine.rs` | 会话授权作为普通 `Rule` 参与，不走特殊通道 |
| 资格检查 `is_allow_eligible` | `permission/eligibility.rs` | 归约前必须先过它（§4.6.1 末段） |
| `set_permission_mode` + `watch::Sender` | `actor.rs:175` | 模式切换按钮直接调它 |

**`set_permission_mode` 已经存在，是这一期最省事的一块。** §6.4 要求"卡片切的模式与指示器切的模式是同一个状态"—— 复用这条通道就天然满足，不要新建第二条路径。

### 必须处理的遗留：删掉 P1 那道 exec 守卫

```rust
// permission/engine.rs:257 —— P1 装的，P2 我明确要求保留
if matches!(effect, Effect::Exec { .. }) && rule.behavior == RuleBehavior::Allow {
    return Some(EvaluatedUnit::ask(...));
}
```

**P3 必须删掉它**，因为会话授权是第一个合法的 exec allow 来源。

删掉之后，`allow exec(cargo test)` 不被 `PATH=. cargo test` 命中的保护**全部落到资格检查身上**。好消息是位置已经对了：

```rust
// engine.rs:188，排在最终 Allow 返回之前
if has_exec && !unit.allow_eligible {
    return EvaluatedUnit::ask(...);
}
```

**但这条链从此只由一个检查守着。** 因此本期必须有一组专门钉住它的测试（§5），而不是顺带覆盖。

## 3. 要建什么

### 3.1 arity 归约（§4.6.1）

一张"命令前缀 → 有意义 token 数"的字典，从最长前缀往回找第一个命中的：

```
git checkout main -b feat  → 命中 git(2)        → exec(git checkout)
npm run dev --silent       → 命中 npm run(3)    → exec(npm run dev)
git config user.name x     → 命中 git config(3) → exec(git config user.name)
cargo test -p foo          → 命中 cargo(2)      → exec(cargo test)
```

**字典可以很小**，规则是"仅当更长前缀的数字不同时才单列"：`git` 是 2，那 `git checkout` / `git commit` / `git diff` 都不用写，只有 `git config` 需要 3 才单列。

三条硬要求：

- **标志位不计入 token**，但这只是归约规则，不是匹配规则。`git -c core.pager=X log` 必须因为 `-c` 在**归约之前**就不合资格（§4.6.1 末段：资格检查在归约之前）；
- **兜底是逐字匹配整条命令，不是取第一个 token。** 退化成第一个 token 会让 `python script.py` 生成 `exec(python)` —— 那正是 §2.4 禁止的；
- 兜底时卡片要说明"仅这一条命令"（验收 29）。

### 3.2 会话授权

**走参数传入，不要塞进 `FinalizedToolset`。**

`PermissionEngine` 现在被 `FinalizedToolset` 持有，后者 finalize 后不可变；会话授权要在会话中途增长。两条路：给引擎加内部可变（`RwLock`），或者让 `authorize()` 多收一个参数、授权由 `SessionActor` 持有。

**选后者。** 它让 §6.4 那句"会话授权只存在于 `SessionActor` 内存、进程重启即消失"从约定变成**结构事实**——想让它跨会话存活，得先改函数签名。这与 `permission_mode` 已有的传递方式也一致。

```rust
// 大意，签名自己定
pub fn authorize(
    &self,
    invocation: &ToolInvocation,
    mode: PermissionMode,
    session_grants: &[Rule],
) -> Authorization
```

两条硬要求（§6.4）：

- 会话授权与文件规则用**同一套匹配器**，不做"输入 JSON 字符串相等"这种判断；
- 会话授权参与 §3.3 合并求值，同样**最严者胜**：内置敏感 `ask` 盖得住它。

### 3.3 按钮选择逻辑（§4.6.3）

> **该按钮提供的会话变更，必须让这次调用的每一个被阻塞单元都获得放行。做不到就不出现这个按钮。**

| 被阻塞的单元 | 按钮 |
|---|---|
| 全是 `exec`，归约可用 | 「本会话允许 `<归约结果>`」 |
| 全是工作区内非敏感 `write` | 「本会话不再询问文件改动（切到 acceptEdits）」 |
| 混合，但切模式后全部能放行 | 同上 |
| 其余 | **不出现**，只留「允许一次」「拒绝」 |

不出现的情况还包括：任一子命令未通过资格检查、命中显式 `ask` 或内置敏感 `ask`、`unparsed`。**不是灰置，是没有这个选项。**

### 3.4 判定表扩充

按用途分批加，**每加一条都要回答"它有没有能变成解释器的标志、有没有藏在里面的写子命令"**：

| 组 | 命令 | 加之前要注意 |
|---|---|---|
| 文本读取 | `head` `tail` `wc` `nl` `tac` `rev` | `tail -f` 会一直挂着——登记时排除 `-f` / `--follow` |
| 搜索 | `grep` | `grep -f FILE` 从文件读模式：要么额外产出 `Read(FILE)`，要么不登记 `-f`（推荐后者） |
| 路径 | `basename` `dirname` `realpath` `stat` `file` | 无 |
| 文本处理 | `sort` `uniq` `cut` `tr` `diff` | `sort -o FILE` **写文件**，不要登记 `-o` |
| 环境 | `whoami` `id` `uname` `hostname` `date` `df` `du` `which` `type` | 无 |
| 无副作用 | `echo` `printf` `true` `false` `seq` | 无 |
| git 只读 | `git log` `git show` `git blame` `git ls-files` `git rev-parse` `git shortlog` `git merge-base` `git describe` `git stash list` `git worktree list` | 见下 |

**git 子命令有两个坑：**

1. **写子命令藏在读命令里。** `git branch -d/-D/-m/-M` 删改分支、`git tag -d` 删标签、`git reflog expire` 写 `.git/logs`。要么整条不登记，要么只登记纯展示标志并拒绝这些形态。P2 已经有 `acc_19` 守着这一类，扩表时要同步扩这条测试。
2. **取值标志的 arity。** `git log -S` / `-G` / `--pretty` / `--format` / `--grep` 都吃一个参数，漏一个就是 §2.3② 那类 parser differential。

**明确不加：** `find`（`-exec` / `-delete` / `-fprintf` 标志面太脏，且已在逃逸标志表里）、`sed`（P4 专项）、`awk`（是解释器，应该加到 §3.5 的清单里而不是判定表）。

### 3.5 补齐解释器/wrapper 清单

`eligibility.rs::is_interpreter_or_wrapper` 目前缺了不少。这张清单同时是 P5 规则语法的拒绝名单（§2.3 第一类），现在补上更省事：

```
解释器   awk gawk mawk php lua tclsh Rscript osascript deno bun
shell    dash ksh fish csh tcsh ash busybox
提权     sudo doas
wrapper  nohup setsid script watch flock chroot ionice taskset
```

## 4. 实现顺序

| # | 内容 | 完成信号 |
|---|---|---|
| 1 | 判定表扩充 + 解释器清单补齐（**不碰会话授权**） | 验收 14 扩面通过；`acc_19` 扩到覆盖 `git branch -d` / `git tag -d`；此时行为只增不减，风险最低 |
| 2 | arity 归约（纯函数，无状态） | 验收 29 通过；`python script.py` 归约不出 `exec(python)` |
| 3 | 会话授权：参数通路 + 合并求值 + 删掉 engine.rs:257 守卫 | 验收 62 通过，**以及 §5 那组资格检查回归** |
| 4 | 按钮选择逻辑 + `PermissionDecision` 扩值 + 桌面端三按钮 | 验收 30、31、54、55、59–66 通过 |
| 5 | Trace：`permissionDecisionSource` 增加 `session_grant`，`permissionModeOrigin` 三取值 | 验收 63、72、73 通过 |

第 1 步单独排在最前面，是因为它**只放宽不收紧、不碰任何机制**，跑完就能独立验证；把它和会话授权混在一个提交里，出问题时分不清是哪边。

第 3 步是全期最危险的一步（删守卫），排在归约之后，是为了让归约的正确性先被测试钉死 —— 归约错了 + 守卫没了 = 提权路径。

## 5. 怎么测

### 必须有的一组：删守卫之后的资格检查回归

P1/P2 期间 `engine.rs:257` 那道守卫在兜底，很多资格检查的测试即使逻辑写错也会因为守卫而通过。**守卫一删，它们才第一次真正生效。**

因此本期必须显式构造"会话授权已存在"的场景重跑一遍：

```rust
#[test]
fn acc_32_session_grant_does_not_survive_a_leading_assignment() {
    let grants = vec![session_grant_exec(["cargo", "test"])];
    // 已「本会话允许 cargo test」
    assert_allows(&toolset, "cargo test -p foo", &grants);
    // 但这些仍然要问
    assert_asks(&toolset, "PATH=. cargo test", &grants);
    assert_asks(&toolset, "RUST_LOG=debug cargo test", &grants);
    assert_asks(&toolset, "timeout 5 cargo test", &grants);
    assert_asks(&toolset, "./cargo test", &grants);
    assert_asks(&toolset, "cd /tmp && cargo test", &grants);
}
```

**这组测试不写，这一期就没有边界。** 逐条对应 §9.2 的 30–37。

### 会话授权盖不住 deny/ask（验收 62）

```rust
// 已「本会话允许 git」后
assert_asks(&toolset, "git push", &[grant_exec(["git"]), rule_ask_exec(["git", "push"])]);
```

### 归约的边界（验收 25、29）

```rust
assert_eq!(reduce("git checkout main -b feat"), TokenPrefix(["git", "checkout"]));
assert_eq!(reduce("npm run dev --silent"),      TokenPrefix(["npm", "run", "dev"]));
assert_eq!(reduce("git config user.name x"),    TokenPrefix(["git", "config", "user.name"]));
// 字典查不到 → 逐字匹配整条，不是第一个 token
assert_eq!(reduce("someunknowntool a b"),       Literal("someunknowntool a b"));
```

### 判定表扩充的对抗面

每加一组命令，配一条"这一组里的写形态仍然要问"：

```rust
assert_asks_in_both_modes(&toolset, "git branch -d feat");
assert_asks_in_both_modes(&toolset, "git tag -d v1");
assert_asks_in_both_modes(&toolset, "sort -o out.txt in.txt");
assert_asks_in_both_modes(&toolset, "grep -f patterns.txt src");
assert_asks_in_both_modes(&toolset, "tail -f app.log");
```

### 前端

按钮三形态各一条；「本会话允许」的文案含归约结果；模式切换按钮的文案**不得提及 bash**（见 §7）。

## 6. 完成的定义

- [ ] `cargo test --workspace` 全绿；clippy 无 warning；`cargo fmt --check` 通过；前端 `vitest run` 全绿
- [ ] `engine.rs` 里不再有 exec 的 Allow 降级守卫
- [ ] §5 第一组（删守卫后的资格检查回归）存在且覆盖 §9.2 的 30–37
- [ ] 会话授权不写文件：跑完一整轮含多次「本会话允许」与模式切换的会话，`~/.openwork/` 下字节数不变
- [ ] 会话授权与模式都随进程重启消失，模式回到 `default`
- [ ] 卡片上不存在任何产生持久规则的按钮；没有「总是拒绝」
- [ ] 判定表新增命令逐条在 PR 里列出，每条附"它有没有逃逸标志/写子命令"的结论
- [ ] PR 描述列出覆盖了 §9.2 的哪些编号、哪些暂缓

## 7. 一条必须守住的文案约束

P4 之前 `acceptEdits` **只打开内置 `allow write(<工作区>/**)`**，七个 bash 文件系统命令与重定向 create 都还没生效。

因此 P3 期间：

- 模式切换按钮的文案**只描述文件工具的写入**，不得提前声称覆盖 bash；
- `mkdir src/x` 一类（§4.6.3 第三行的混合场景）**不出现中间按钮**——切过去也解不开。

文案随 P4 一并扩写。这条已写进 [permissions.md §9.1](permissions.md)，**否则第一版就违反 §5"卡片不得声称超出实际的边界"**。

## 8. 最容易做错的四处

**① 删守卫时顺手把资格检查也"简化"了。**
守卫（`engine.rs:257`）和资格检查（`engine.rs:188`）看起来在做同一件事，实则一个是粗暴兜底、一个是真正的门。删前者、动后者，这一期就白做了。

**② 归约先于资格检查。**
必须反过来。`git -c core.pager=X log` 若先归约成 `git log` 再查资格，`-c` 就丢了。**资格检查在归约之前**（§4.6.1）。

**③ 给 `write` 类卡片做路径范围授权。**
不得出现 `[本会话允许 write(src/**)]`。中间粒度是用户在打断时刻判断不了的范围，看起来精确反而更危险（§4.6.2）。`write` 类只给模式切换。

**④ 扩判定表时按"这命令听起来是只读的"下判断。**
`git branch` 听起来是只读的，`git branch -d` 不是。`sort` 听起来是只读的，`sort -o` 不是。**标志白名单是封闭性的唯一来源**，程序名不是。

## 9. 交付物

1. 代码 + 测试（§5 第一组是本期的重心）
2. PR 描述：§9.2 编号覆盖/暂缓清单 + **判定表新增命令逐条结论**
3. 实现中若发现 permissions.md 有做不到或自相矛盾的条款，在 PR 里单独列出，不要自行绕过

**动手前先给实现方案**，重点说明会话授权的传递方式（§3.2）与删守卫后的保护路径（§2 末段）。
