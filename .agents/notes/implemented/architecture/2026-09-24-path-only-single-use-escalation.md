# Agent Note: 越界只列具体路径，只批这一次

Status: implemented

## 问题

沙箱会拒绝一些正当的操作。新增依赖后首次下载要写 `~/.cargo/registry`，`git push` 要读 `~/.ssh`，`git commit` 要写 `.git`。系统需要一条出路。

出路的宽度决定用户在卡片上判断什么。如果越界能给出“任意位置可写”，这一次执行中的 `build.rs` 就能写任何地方。出路的生存期也有同样的问题。批准如果累加到会话里，用户点下时判断的是眼前这一条，授权却覆盖以后的调用。

## 决策

- 越界只能列出具体路径。`sandboxPermissions` 只有 `paths`，每条是路径、`read` / `write`、`exact` / `subtree`（`crates/openwork-tools/src/escalation.rs` 的 `EscalationInput`，带 `deny_unknown_fields`）。bash、`write`、`edit` 都带这组参数。
- `SandboxMode` 只有两个取值，越界目标也没有完全放开的取值。
- 授权只作用于这一次调用。`authorize_tool_call`（`crates/openwork-core/src/session/run_loop/authorization.rs`）只为这次执行构造带授权的策略。会话状态不持有授权。
- 模型可以不等拒绝就请求越界，参数说明写明了这一点。系统不把请求匹配到先前的拒绝。卡片只附上同一 Turn 上一次被拒的那一行（`ApprovalCard.previous_denial`）。
- `SandboxPolicy::validate_grants`（`crates/openwork-sandbox/src/policy.rs`）拒绝落在 `/`、`$HOME` 或其祖先上的 `subtree`（`is_too_broad`），最多 `MAX_GRANTS = 16` 条。校验失败时不出卡片。

设计见 [permissions.md](../../../../docs/subsystems/permissions.md)。

## 考虑过的方案

**越界给出更宽的模式。** DSH 的越界目标是模式：`workspace-write` 或 `danger-full-access`（`packages/sandbox/sandbox/src/escalation.ts` 的 `ESCALATION_TARGETS`）。maka 有 `bypass` 边界（`packages/core/src/sandbox-boundary.ts` 的 `ExecutionBoundary`），Codex 有 `danger-full-access`。没有采用：只列路径时，越界范围等于真正需要的范围。写入位置列不出来的命令不在能力范围内，交给用户执行。

**批准的路径累加进会话边界。** maka 的越界也是路径形态，但批准会扩大会话的 `ExecutionBoundary`，之后一直有效（`packages/core/src/sandbox-boundary.ts` 的 `applySandboxBoundaryExpansion`）。没有采用：用户判断得了眼前的命令，判断不了这条授权以后覆盖什么。本设计取 maka 的路径形态与 DSH 的单次生存期。

**「本会话允许」与持久规则。** 2026-08-01 的设计有会话授权：卡片上的「本会话允许 `<归约后的命令前缀>`」。同一份设计拒绝了持久规则文件。Codex 的“总是允许”写入 `default.rules`（`codex-rs/core/src/exec_policy.rs` 的 `DEFAULT_POLICY_FILE`）。现在两者都不做。是否需要会话级授权，交给 [按 Trace 数据决定](../../proposed/architecture/2026-09-24-trace-driven-cache-and-session-grants.md)。

**把重试硬匹配到先前的拒绝。** 没有采用：命令字符串的同一性很脆弱，引号、`workdir`、环境变量前缀都会改变它。硬匹配要么误拒诚实的重试，要么很容易满足。真正的门控是用户看到的命令、路径与理由。

## 后果

- 卡片上的范围容易看懂：逐条路径、读或写、单个文件或子树、所属的档。
- `escalationPaths` 只有路径形态，可以直接按路径聚合。
- 代价：同一条命令下次还需要越界，就再问一次。提交类 git 操作每次都要批准。
- 代价：写入位置列不出来的安装脚本，模型执行不了。
- `$HOME` 下的具体目录（如 `~/.cargo/registry`）仍可按子树授权。
- 撤销与重新应用文件改动使用同样的单次精确授权，见 [撤销与重新应用文件改动时的授权](2026-09-24-undo-reapply-grants.md)。
- 测试：core `acc_18_19_an_escalation_asks_with_its_paths_and_applies_to_that_call_only`、`acc_22_an_escalation_that_fails_validation_is_refused_without_a_card`。
