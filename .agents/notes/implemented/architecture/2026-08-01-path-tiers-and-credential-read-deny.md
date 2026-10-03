# Agent Note: 路径分四档，凭据目录禁读

Status: implemented

## 问题

可写根决定 bash 能写哪里。可写根内仍有一些位置，写进去就变成沙箱外的执行，或改变 Agent 自己的运行环境。可写根外有一些位置，读到就等于拿到登录态。系统不管控网络，沙箱内能读到的内容理论上都能发出去。

所以要有一组规则说明：哪些路径永远不可写，哪些写入要用户确认，哪些路径不可读。

## 决策

- `crates/openwork-sandbox/src/tiers.rs` 是唯一持有清单的地方。`SandboxPolicy::tier`（`policy.rs`）返回 `PathTier`：`HardProtected`、`Sensitive`、`Credential`、`Normal`。
- 硬保护：`~/.openwork`、skill 根（`SandboxEnvironment::hard_protected_roots`），以及工作区内任意深度的 `.git/hooks`（`is_workspace_hard_protected`）。任何授权都解不开它们，Seatbelt 以 `require-not` 扣除。
- 敏感：`.git` 的其余部分、`.env*`、`.vscode`、`.idea`、shell 配置文件等。可读，写入要越界。
- 凭据：`CREDENTIAL_PATHS`，包括 `~/Library/Cookies`、`~/Library/Safari` 与 Chrome、Firefox 的配置目录。不可读，越界点名后可读。
- 授权只解开它自己点名的敏感或凭据路径（`SandboxPolicy::grant_unlocks`）。授权整个工作区，不会打开 `.git` 与 `.env`。
- profile 从 `(allow default)` 与 `(deny file-write*)` 开始，只对凭据路径加 `deny file-read*`（`seatbelt.rs` 的 `SeatbeltProfile::new`）。
- 名字按 ASCII 大小写不敏感匹配，见 [工作区内的档位名按 ASCII 大小写不敏感匹配](2026-09-24-case-insensitive-protected-names.md)。

公开接口见 [openwork-sandbox README](../../../../crates/openwork-sandbox/README.md)。

## 考虑过的方案

**整个 `.git` 硬拒绝。** 2026-08-01 的设计硬拒绝 `<工作区>/**/.git/**` 与 `<工作区>/**/.openwork/**`，理由是“可被批准的边界不是边界”。没有保留：`git add`、`commit`、`stash` 都要写 `.git`，硬拒绝就等于模型永远不能提交。Codex 把整个 `.git` 设为只读（`codex-rs/protocol/src/permissions.rs`）。DSH 的 profile 没有受保护子路径（`packages/sandbox/sandbox-local/src/profiles.ts`）。maka 支持保护 `.git`，但默认的 `createWorkspaceWritePermissionProfile` 不启用它（`packages/core/src/permission-profile.ts`）。

现在硬保护只收一类路径：一次批准就变成沙箱外的持久执行，并且没有正当用途。`hooks` 里的脚本会在用户下一次 `git commit` 时在沙箱外执行。`~/.openwork` 存着 Agent home 与 runtime token。改 skill 能把一次提示注入变成跨 Session 的持久提权。

**敏感档不可由任何东西降级。** 2026-08-01 的设计让敏感路径始终出卡片，会话授权盖不住。现在越界可以解开它：越界只作用于一次调用，只覆盖点名的路径。

**默认拒绝（deny default）。** maka（`packages/runtime/src/sandbox/macos-seatbelt.ts`）与 Codex（`codex-rs/sandboxing/src/seatbelt_base_policy.sbpl`）从 `(deny default)` 开始，读取限于工作区与系统目录。没有采用：读依赖源码与工具链不应被打断。deny default 的允许清单还要随系统版本维护。接入网络管控时再重新评估。

**敏感档也禁读。** 没有采用：保护目标是不可篡改，不是保密。`.env` 在工作区里，模型读它是编码工作的一部分。

## 后果

- 凭据禁读比 DSH 与 Codex 的默认档更紧。网络不管控时，它把最值钱的目标从“能读到”里拿掉。
- 浏览器 Cookie 与配置目录算凭据，因为它们存着已登录的会话。
- 越界能解开凭据档。`git push` 要读 `~/.ssh`，`gh pr create` 要读 `~/.config/gh`。卡片标出路径所属的档。
- 代价：提交类 git 操作每次都要批准一次。
- 代价：凭据清单是枚举的。清单外的凭据位置仍然可读。
- 测试：`crates/openwork-sandbox/tests/matrix.rs` 的 `protected_locations_stay_closed_and_temp_stays_open`；tools `sandbox_calls::acc_10b_credential_directories_are_unreadable_for_file_tools_and_bash`。
