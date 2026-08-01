# 权限系统 P4 实施 brief

> 交接文档，P4 落地后可删除。P1–P3 的 brief 此时可以一并删掉。
>
> 规格的唯一事实来源是 [permissions.md](permissions.md)。两者冲突时以它为准。

## 0. 一句话

让 `acceptEdits` 名副其实 —— 但**第一步是拆结，不是加功能**。

## 1. 本期边界

### 做

对应 [permissions.md §9.1](permissions.md) 的 P4：

1. **拆结**（§4.5 末段）—— 只读判定从"返回终局判定"改成"产出证据、参与合并求值"
2. **重定向 create** —— 见 §3.1：**这一条是第 1 步的副产品，不需要单独实现**
3. **七个文件系统命令闸门**（§4.8）—— `mkdir touch rm rmdir mv cp sed`
4. **`sed` 专项**（§4.8 末段）—— 脚本子集校验，且与只读判定共用同一个校验器
5. 解除 P3 的临时抑制与文案约束（§6）

### 不做

| 能力 | 阶段 |
|---|---|
| `permissions.toml` 加载、用户手写规则 | P5 |
| 规则管理界面、会话授权导出为规则 | P6 |

### 本期结束后的预期行为

```
acceptEdits:  mkdir src/x · touch src/a.ts · rm src/tmp.txt · cp a b
              sed -i 's/x/y/' src/a.ts · echo x > src/a.txt        → 自动执行
              rm -rf .           → 问（效果是 write(<工作区>) 本身）
              rm -rf .git        → 直接拒绝，不出卡片
              rm .env            → 问（内置敏感 ask）
              mkdir ../outside   → 问（工作区外）
              chmod / ln / tee / git commit / npm install          → 问
default:      以上文件系统命令全部要问，但卡片给的是模式切换按钮
```

## 2. 第一步：拆结

### 现状

`engine.rs::evaluate_unit` 里，只读判定返回的是**终局判定**而不是证据：

```rust
if has_exec && unit.readonly_proof.is_some() {
    if unit.effects.iter().any(|e| matches!(e, Effect::Write { .. })) {
        return EvaluatedUnit::ask(...);        // ← 终局，preempt 掉后面的规则求值
    }
    let all_reads_allowed = ...;
    if all_reads_allowed { return EvaluatedUnit { verdict: Allow, ... }; }
}
```

它排在"所有效果都被规则覆盖 → 取最严"**之前**，后果是：

```
allow exec(echo)  +  echo x > src/a.txt   →  永远 Ask，规则静默失效
```

用户只会觉得自己写的规则没用，而且没有任何提示。P5 让用户手写规则之后这会变成一类反复出现的报错。

### 目标形状

把判断拆成两层：**每个效果各自找放行证据，然后单元层要求全部有证据。**

```
每个效果的放行证据来源（并列，任一命中即可）：
  Exec   ← 规则 allow（含会话授权） | 只读判定 | 文件系统命令闸门（acceptEdits）
  Read   ← 规则 allow | 内置 allow read(<ws>/**)
  Write  ← 规则 allow | 内置 allow write(<ws>/**)（仅 acceptEdits）

单元判定：
  1. 规则层：任一效果命中 deny / ask → 直接返回（§3.3 最严者胜）
  2. 有 exec 且资格检查未通过 → Ask
  3. 每个效果都拿到放行证据 → Allow
  4. 否则 → Ask（source = NoRuleCovers）
```

这与 §4.1 的流水线图一致：**规则层在前，内置证据在后，内置证据之间并列、无顺序。**

### 拆完之后，重定向 create 自动就有了

`echo x > src/a.txt` 在 `acceptEdits` 下：

| 效果 | 证据 |
|---|---|
| `Exec { echo }` | 只读判定 |
| `Read(<ws>)` | 内置 `allow read` |
| `Write(<ws>/src/a.txt)` | 内置 `allow write`（模式打开） |

三个都有证据 → Allow。**§3.5 说的"重定向 create"不是一个独立机制**，它只是重定向产生的 `Write` 效果被 `allow write(<工作区>/**)` 覆盖——而 `Write` 效果 P1 就已经提取好了（`bash/mod.rs::analyze_redirects`）。

所以验收 27、28 在第 1 步做完就该变绿，不要为它们单独写代码。

### 风险

这一步动的是**所有工具、所有模式都要经过的那段代码**，P1–P3 的行为全部依赖它。

**要求：第 1 步做完先跑一遍全量回归，确认 P1–P3 的 acc 编号一条都没变红，再开始第 2 步。** 如果某条变红，先判断是回归还是"这条测试原本依赖短路才通过"——后者要连同测试一起修正并在 PR 里说明。

## 3. 文件系统命令闸门（§4.8）

**闸门的职责是把命令翻译成效果，翻译不出来就不放行。** 翻译完成后效果照常过 §4.1 的流水线——它们不是"已经批准的"，只是"已经知道是什么"。

| 命令 | 产出效果 | 登记要点 |
|---|---|---|
| `mkdir` | 每个操作数 `Write(p)` | `-p`；`-m MODE` **吃一个参数** |
| `rmdir` | 每个操作数 `Write(p)` | `-p` |
| `touch` | 每个操作数 `Write(p)` | `-a` `-m` `-c`；`-r FILE` 额外产出 `Read(FILE)` |
| `rm` | 每个操作数 `Write(p)` | `-r` `-R` `-f` 及组合 |
| `mv` | `Write(源)` + `Write(目标)` | 见下 |
| `cp` | `Read(源)` + `Write(目标)` | 见下（**源按 read，这是与 cc-haha 的有意差异 #9**） |
| `sed` | `-i` → 每个文件操作数 `Write(p)`；无 `-i` → 交给只读判定 | 见 §4 |

### 三个必须接住的陷阱

**① `-t` / `--target-directory` 会调换操作数顺序。**

```
mv -t dest a b        目标是 dest，不是 b
cp --target-directory=dest a b
```

按"最后一个操作数是目标"的直觉解析，`mv -t dest a b` 会把 `b` 当目标、`dest` 当源——效果集完全错位。这与 §2.3② 的 `git diff -S` 是同一类失效（判定器与真实程序对同一串 argv 理解不同）。

**要么正确实现 `-t` 的语义，要么把它登记成不放行。** 推荐后者：P4 不需要支持它。

同类还有 `cp -T` / `--no-target-directory`（禁止把目标当目录）。

**② 未登记的标志一律不放行。** 和只读判定表同一条规矩（§2.3②）。`mv --backup=numbered`、`cp --parents`（会创建中间目录！）、`rm --one-file-system` 这些没登记就不放行，不要因为"看起来无害"而放行。

**③ 工作区根本身。** `rm -rf .` / `rm -rf ./` 的效果是 `Write(<工作区>)`，落不进 `write(<工作区>/**)`（§3.4）。P1 已有 `workspace_root_is_not_covered_by_workspace_descendant_rules` 守着这条，P4 要补一条端到端的（验收 40）。

## 4. `sed` 专项

`sed` 是七个里唯一能变成解释器的，单独一节。

### 一个校验器，两个入口

§8 差异 #8 承诺了：只读判定与 `sed` 闸门**并列**，所以 `sed -n '1p' file` 应该在 `default` 下就由只读判定放行（cc-haha 在这里的行为是反的，我们有意不跟）。

但 `sed` 目前**不在只读判定表里**，而它不能像 `cat` 那样直接登记——因为不带 `-i` 的 sed 照样能写文件、能执行命令：

```
sed 'w /tmp/x' file      写任意文件，没有 -i
sed 'e id' file          执行命令
sed 'r /etc/passwd' file 读任意文件
```

所以：**写一个 sed 脚本校验器，只读登记和 `-i` 闸门共用它。** 两个入口对脚本的要求相同，区别只在产出的效果（只读路径产出 `Read`，`-i` 路径额外产出 `Write`）。

### 脚本必须落在这个子集内

- `[地址]s/…/…/flags` 与 `[地址]d`，可用 `;` 或多个 `-e` 串联，**每一段都要能识别**；
- 出现 `w` `W` `r` `R` `e` `E` `F` 任一动作即不放行；
- `-f scriptfile` 不放行——脚本内容在文件里，看不见；
- `-i.bak` 这类备份后缀**额外产出 `Write(file.bak)`**；
- 分隔符不限于 `/`：`s|a|b|`、`s#a#b#` 都合法，**解析必须按 sed 的真实规则**，包括转义分隔符 `s/a\/b/c/`。

**分隔符解析出错就是一次 parser differential**，按 §4.7 fail-closed 处理。这一条建议单独写一组表驱动测试。

## 5. Trace

`permissionDecisionSource` 增加取值 `mode_fs_command`。

**多来源时记哪一条？** 一个单元的效果可能各自来自不同证据（`echo x > f` 的 exec 来自只读判定、write 来自模式）。记录**决定性的那一条**——去掉它这次调用就不会自动放行的那条；多条都决定性时按这个顺序取第一个：

```
mode_fs_command > mode > session_grant > readonly_proof > rule > builtin
```

理由：排在前面的是"最近授予、最容易被质疑"的证据，事故复盘时首先要看的就是它。

## 6. 解除 P3 的两处临时措施

**① 删掉 `engine.rs::is_p4_filesystem_command`。**

P3 用它阻止为七个命令发 `exec` 会话授权，理由是"P3 只能装 exec 前缀授权，而这类命令的写效果那时还提取不出来"。P4 之后这个理由消失。

删掉之后不需要额外做什么：`default` 下 `mkdir src/x` 的按钮会**自动**变成模式切换——因为 §4.6.3 的判断是真跑一遍 `authorize_internal(AcceptEdits, …)`，而 P4 之后那一跑会返回 `Allow`。这正是 §4.6.3 表格第三行。

**顺带确认**：`exec(mkdir)` 的会话授权在 `default` 下仍解不开 `Write(src/x)`（模式没开），所以 dry-run 不会通过，不会退回去发 exec 授权。写一条测试钉住这个。

**② 文案扩写。** `enableAcceptEdits` 当前是"本会话不再询问**文件工具**改动" —— P3 的临时收窄。P4 后按 §4.6.2 改成覆盖**本会话内工作区所有非敏感写入，包括 bash 的七个文件系统命令与输出重定向**。

三个语言包都要改（`zh-CN` / `zh-TW` / `en-US`），[permissions.md §9.1](permissions.md) 里那条 P3/P4 文案约束到此失效。

## 7. 实现顺序

| # | 内容 | 完成信号 |
|---|---|---|
| 1 | **拆结** | 全量回归绿；验收 27、28 变绿（重定向 create 白送）；`allow exec(echo)` + `echo x > f` 在 `default` 下自动执行 |
| 2 | 六个命令闸门（不含 `sed`） | 验收 38–43、47 通过 |
| 3 | `sed` 校验器 + 两个入口 | 验收 44、45、46 通过 |
| 4 | 删 `is_p4_filesystem_command` + 文案扩写 + trace `mode_fs_command` | 验收 64 的按钮形态变成模式切换；三个语言包一致 |

第 1 步单独跑完再往下，理由见 §2 末段。

## 8. 怎么测

### 拆结之后的回归（第 1 步的重心）

不是新写测试，是**跑一遍已有的全部 acc 编号**并逐条确认没变红。变红的要分清是回归还是"原本靠短路才通过"。

新增一条正面用例：

```rust
#[test]
fn acc_27_readonly_command_with_redirect_is_allowed_by_a_rule() {
    // default 模式 + 显式 allow exec(echo) + allow write(<ws>/**)
    // 拆结前：永远 Ask；拆结后：Allow
}
```

### 闸门的对抗面

每个命令配一条"这个形态仍然要问"：

```rust
assert_asks(&toolset, "mv -t dest a b",        AcceptEdits);  // -t 未登记
assert_asks(&toolset, "cp --parents a b/c",    AcceptEdits);  // 会建中间目录
assert_asks(&toolset, "rm --one-file-system x", AcceptEdits); // 未登记标志
assert_asks(&toolset, "mkdir -m",              AcceptEdits);  // -m 缺参数
assert_asks(&toolset, "rm -rf .",              AcceptEdits);  // 工作区根
assert_asks(&toolset, "rm -rf ./",             AcceptEdits);
assert_denies(&toolset, "rm -rf .git",         AcceptEdits);  // 硬 deny，不出卡片
assert_asks(&toolset, "rm .env",               AcceptEdits);  // 内置敏感
assert_asks(&toolset, "mkdir ../outside/new",  AcceptEdits);  // 工作区外
assert_asks(&toolset, "cp /etc/passwd src/x",  AcceptEdits);  // 源的 read 在外
```

### sed 的分隔符表

```rust
let cases = [
    ("sed -i 's/a/b/' f",         Provable),
    ("sed -i 's|a|b|' f",         Provable),
    ("sed -i 's#a#b#' f",         Provable),
    (r"sed -i 's/a\/b/c/' f",     Provable),   // 转义分隔符
    ("sed -i 'w /tmp/x' f",       NotProvable),
    ("sed -i 'e id' f",           NotProvable),
    ("sed -i 's/a/b/;w /tmp/x' f", NotProvable), // 串联里藏动作
    ("sed -i -f script.sed f",    NotProvable),
    ("sed -n '1p' f",             Provable),   // 只读路径，default 下即放行
];
```

### 前端

模式切换按钮的新文案在三个语言包一致，且**确实提到 bash 的七个命令与重定向**（§4.6.2）——P3 那条"不得提及 bash"的约束在这里反向生效。

## 9. 完成的定义

- [ ] `cargo test --workspace` 全绿；clippy 无 warning；`cargo fmt --check` 通过；前端 `vitest run` 全绿
- [ ] `engine.rs` 里只读判定不再返回终局判定，改为产出证据
- [ ] `is_p4_filesystem_command` 已删除
- [ ] 拆结后 P1–P3 的 acc 编号无一变红（变红的在 PR 里逐条说明）
- [ ] `enableAcceptEdits` 文案三语言包一致，且覆盖 bash 侧
- [ ] 闸门新增命令逐条在 PR 里列出，每条附"未登记的标志/会调换操作数的标志"的结论
- [ ] PR 描述列出覆盖了 §9.2 的哪些编号、哪些暂缓

## 10. 最容易做错的四处

**① 把重定向 create 当成一个要写的功能。**
它是拆结的副产品（§2 末段）。如果发现自己在为它写新代码，说明结没拆干净。

**② 拆结时把"证据"和"判定"又混起来。**
只读判定不再回答"这个单元的结论是什么"，只回答"`Exec` 这个效果有没有放行依据"。单元结论由"所有效果是否都拿到证据"决定。

**③ `mv -t dest a b` 按"最后一个是目标"解析。**
效果集会完全错位。不支持就登记成不放行，不要猜（§3①）。

**④ 把 `sed` 当成 `cat` 那样登记进只读判定表。**
不带 `-i` 的 sed 照样能 `w` 写文件、`e` 执行命令。只读登记和 `-i` 闸门必须共用同一个脚本校验器（§4）。

## 11. 交付物

1. 代码 + 测试（第 1 步的回归确认是本期重心）
2. PR 描述：§9.2 编号覆盖/暂缓清单 + 闸门命令逐条结论 + **拆结后变红的测试及原因**
3. 实现中若发现 permissions.md 有做不到或自相矛盾的条款，在 PR 里单独列出，不要自行绕过

**动手前先给实现方案**，重点说明拆结后的证据模型形状（§2）与 `sed` 校验器的边界（§4）。
