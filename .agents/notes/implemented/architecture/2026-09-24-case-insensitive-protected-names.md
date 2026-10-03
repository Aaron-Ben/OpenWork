# Agent Note: 工作区内的档位名按 ASCII 大小写不敏感匹配

Status: implemented

## 问题

macOS 的默认文件系统 APFS 不区分大小写。`.ENV` 与 `.env` 是同一个文件，`.GIT/hooks` 就是 `.git/hooks`。规范化只能把已存在的路径段还原成磁盘上的写法。新建的路径保留调用方给的拼写。

如果按字面比较档位名，模型新建一个 `.ENV` 就绕过了敏感档。dotenv 一类的工具仍会把它当 `.env` 读。这个绕过对文件工具围栏与 Seatbelt profile 同样成立。

## 决策

- `crates/openwork-sandbox/src/tiers.rs` 是唯一持有档位清单的地方。`is_workspace_hard_protected` 与 `is_workspace_sensitive` 用 `eq_ignore_ascii_case` 比较 `.git`、`hooks`、`SENSITIVE_DIRECTORIES`、`SENSITIVE_FILES` 与前缀 `SENSITIVE_FILE_PREFIX`（`.env`）。
- Seatbelt 的正则没有大小写不敏感的开关。`case_insensitive` 把每个 ASCII 字母展开成字符类，例如 `.env` 变成 `\.[eE][nN][vV]`。`workspace_hard_protected_regex` 与 `workspace_sensitive_regexes` 用它生成传给 `-D` 的正则。
- 规则只作用于工作区内的档位名。`~/.openwork`、skill 根与凭据目录按规范化后的路径匹配。
- `crates/openwork-sandbox/tests/parity.rs` 的 `case_variants_of_protected_names_are_not_writable` 在真实内核上检查大小写变体：内核与围栏都拒绝写入。

设计见 [permissions.md §3](../../../../docs/subsystems/permissions.md)。

## 考虑过的方案

**按字面匹配档位名。** 这是最直接的写法，也是 Seatbelt 正则的默认行为。没有采用：新建的 `.ENV` 会落在敏感档之外，而在磁盘上它就是 `.env`。

## 后果

- 档位清单只有一份。围栏与 profile 由同一组常量生成，对等测试把两侧绑在一起。
- 在区分大小写的卷上，`.Env` 与 `.env` 是两个文件，`.Env` 也按敏感档处理。代价只是多一次越界申请，边界不会变宽。
- 只折叠 ASCII 字母。清单里的名字都是 ASCII，所以不需要 Unicode 大小写折叠。
- profile 的正则参数变长，但 profile 的结构不变。
