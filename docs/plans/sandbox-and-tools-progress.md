# 进度：沙箱权限与工具改造

配合 [sandbox-and-tools.md](sandbox-and-tools.md) 使用：计划写"做什么、怎样算完成"，本文写"做到哪了、定了什么"。每完成一步就更新；计划删除时一并删除。

分支：`permission`（基于 `0b44333`）。

## 1. 工作包状态

| 工作包 | 状态 | 说明 |
|---|---|---|
| WP0 基线 | 完成 | 基线检查的两处问题见 §2 D2；回放数字见 §4 |
| WP1 结果有界与落盘 | 完成，已提交 | `f9db450` |
| WP1b 旧结果修剪 + 先读后改 | 完成，已提交 | `f9db450`（与 WP1 同一提交）；含迁移 `202609240001_add_tool_result_pruning_watermark.sql` |
| WP2 `openwork-sandbox` crate | 完成，已提交 | `6d9081f`；D4、D6、主目录工作区均已实现并有测试（§2）；`GOCACHE` 由 crate 给出，bash 启动时实际设置在 WP3 |
| WP3 沙箱切换 | 完成（收尾待提交） | 提交一 `4d65def`、提交二 `c7a734f`；收尾：`openwork-sandbox` 注释改为中文，文档待定项已改（`nesting_too_deep` 入 §4.3、tools.md §12 #7 重写），验收对照见 §5，§4.2 手动场景用户确认通过（2026-09-24） |
| WP4 工具 T1 | 未开始 | |
| WP5 工具 T2 | 未开始 | |
| WP6 后台任务 | 未开始 | 开始前先定 D8 |
| WP7 收尾 | 未开始 | |


## 2. 已定决策

结论不再重议，除非出现新事实。

| # | 结论 |
|---|---|
| D1 | 同意 §2 的顺序；在 `permission` 分支上工作，不用 worktree |
| D2 | 修复 trace 保留测试（其他测试的旧时间戳 span 会被一并清理，断言改为 `>= 4`）；collab 的 `reported_rate_limit_terminates_a_still_running_opencode_process` 记为已知不稳定，带着走；参照任务用确定性回放测量 |
| D3 | WP1 后模型可见 token 只降 4%，把旧结果修剪与先读后改提前为 WP1b，然后再做沙箱 |
| D6 | read 与落盘阈值从 50 KB 降到 **32 KB**（32,000 字节，含脚注），与请求投影上限 8000 token × 4 对齐 |
| D6 | 单个落盘文件上限 **64 MB** |
| 设计 | 删除内部的 ReadOnly 模式，只保留 `auto` 与 `accept-edits`；explorer 的沙箱上限为 `accept-edits` |
| 设计 | 工作区内的敏感 / 硬保护文件名按 ASCII 大小写不敏感匹配（APFS 默认大小写不敏感，新建的 `.ENV` 否则可绕过） |
| 设计 | 工作区为 `$HOME` 或其祖先时，bash 在两个模式下都不能写工作区，文件工具不受影响；不采用"把自启动路径加入硬保护"（列举不全）。permissions.md §2.2、§9.2 #49（2026-09-24） |
| D4 | bash 启动时把 `GOCACHE` 设为 `$TMPDIR` 下 OpenWork 私有的目录（`<临时根>/openwork/go-build`），不把 `~/Library/Caches/go-build` 加入可写根。permissions.md §3.1、§9.2 #50（2026-09-24） |
| D6 | 可写设备清单为 `/dev/null`、`/dev/zero`、`/dev/tty`、`/dev/fd/<n>`、`/dev/ttys<n>`；删去 `/dev/stdout`、`/dev/stderr`（符号链接，Seatbelt 按解析后的 `/dev/fd/1`、`/dev/fd/2` 判断，单列不起作用）。对照：Codex 标准档只放行 `/dev/null`，这些写法在 Codex 里同样失败。permissions.md §3.1（2026-09-24） |
| — | Go 模块缓存保持现状：`~/go/pkg/mod` 不加入可写根、不重定向，首次下载依赖时由模型申请越界；已缓存的模块只读，不受影响（2026-09-24） |
| — | `openwork-sandbox` 的英文注释在 WP3 改到对应文件时改为中文（comments.md §1），不单独提交（2026-09-24） |
| — | 越界参数改为驼峰 `sandboxPermissions`（与 contracts.md 的工具参数命名一致），`justification` 不变；设计文档与 `openwork-sandbox` 的提示文本已同步（2026-09-24） |
| D7 | 界面先用静态 HTML 确认设计，再写组件；§4.2 手动场景由用户在真实 Desktop 中执行（2026-09-24） |
| D7 | 静态原型确认（https://claude.ai/artifact/6HTXL7wV16txRnTvAfsaG1）：模式名"自动"/"只让编辑工具改文件"；越界卡片"允许一次"为主按钮，危险命令卡片"拒绝"为主按钮；Trace 七类标签配色按原型。写组件前重新读画布（用户在画布上有编辑）（2026-09-24） |

| — | `SessionUpdate::PermissionResolved` 删去 `permissionMode`：卡片不再切换模式，这个字段没有来源；Session Update 与快照共用 `SESSION_UPDATE_VERSION = 7`（2026-09-24） |
| — | 子 Agent 的模式是派生时的快照，`set_permission_mode` 对子 Agent 返回 `SubAgentModeFixed`，不改库也不改 actor（permissions.md §6.6）（2026-09-24） |
| — | 快照带 `sandbox: SandboxStatus`（`{state: available}` / `{state: unavailable, reason}`），取自工具集实际使用的后端，不另存一份（2026-09-24） |
| — | 撤销 / 重新应用文件改动：会话模式 + 这批改动涉及的每个文件的精确写授权，硬保护仍不可写（用户选 A）。permissions.md §2.4（2026-09-24） |
| — | 沙箱不可用时不做危险命令检测、不出卡片（以 §9.2 #13 为准，§2.1 流程图改为先判断沙箱可用）；`control_tool` 补进 §7 的 `permissionDecisionSource` 取值与 §6.4 分类（review-branch，2026-09-24） |
| — | 迁移 `202609240002` 给已有子 Agent 会话回填 `accept_edits`（explorer 的上限），根会话取 `auto`；本地测试库已重置该迁移记录（review-branch，2026-09-24） |
| — | Trace 时间线类别：§6.4 的七类，另加 `control_tool`、`cancelled`（等待审批时 Turn 被取消）、`unknown`（属性缺失或旧版本 Trace，显示"来源未知"）；执行了却被内核拒绝的调用即使经过用户批准也归"被沙箱拒绝"（2026-09-24） |
| — | 模式指示器在 Turn 运行中不再禁用（permissions.md §6.1 "随时可切，下一次调用生效"）；原先运行中禁用是旧设计遗留（2026-09-24） |
| — | `runtime/sandbox-policy` 正文为 `<sandbox_policy>` 包裹的四行：mode、workspace、write / edit、bash 能写什么（或不可用说明）；排在 world state 四个 section 的最后，不改变前三个的字节（2026-09-24） |

## 3. 待定项

| # | 事项 | 现状与建议 |
|---|---|---|
| — | 工具改进（对照 ZCode 等得出，暂不做） | read 重复读取去重；grep 默认只列文件并支持 `-A/-B/-C`；按时间或空闲修剪旧结果；edit 行号前缀与弯引号容错（并入 WP4）；超限预览缩小；先看 Trace 缓存命中数据再决定 |
| — | multi-agent.md 过时段落（与本计划无关，只记录） | 说 `last_real_user`"必须改"、"现在"会选中子 Agent 消息，并链接到已不存在的 `compaction/projection.rs`；代码已在 `compaction/compacted_view.rs:26` 按 kind 排除 contextual 消息。需要改写该段与链接 |
| — | tools.md 出处错误 | "bash 保留头 2 KB + 尾 14 KB 来自 DSH"不对，DSH 只保留尾部。WP7 时改 |
| — | `session_runtime.rs` 超出测试文件上限 | 5226 行（上限 1500，本次从 5362 行减少）。拆分需要共享夹具模块，而每个测试二进制只用其中一部分会触发 `dead_code`，与"禁止 `#[allow(dead_code)]`、禁止模块级抑制"冲突；需先定共享测试夹具的 lint 规则 |
| — | `postgres_session_storage.rs` 超出测试文件上限 | 1529 行，本次为新迁移加了 5 行断言 |


## 4. 参照任务回放

对 `0b44333` 的冻结快照按固定顺序执行同一组工具调用；token 按 4 字节 / token 估算，"投影后"指经请求投影单条 8000 token 上限后的值。

| 时点 | 原始 token | 投影后 token |
|---|---|---|
| WP0 基线 | 84,650 | 42,927 |
| WP1 之后 | 41,251 | 41,251（−4%） |

WP1 的收益主要是正确性：不再从中间截断、总数准确、完整结果可取回。WP4 后重跑并补一行。

## 5. WP3 验收对照

测试名省略 crate 前缀：`sandbox` = `openwork-sandbox`，`tools` = `openwork-tools`，`core` = `openwork-core`，`desktop` = `desktop/src`。"手动" 指用户在真实 Desktop 中执行的 §4.2 场景（2026-09-24，用户确认无问题）。

### permissions.md §9.2

| # | 证据 |
|---|---|
| 1 | core `postgres_core_host_flow::acc_01_35_42_…`（新会话 `auto`）、`session_runtime::acc_01_13_…`；`SandboxMode` 只有两个变体（sandbox `policy::modes_order_from_narrow_to_wide`）；desktop `ChatInput.test`；常驻可见：手动 |
| 2 | sandbox `matrix::auto_builds_and_reads_but_git_writes_need_an_escalation`（`cargo build/test`、`git status/log/diff` 在内核下成功）；core `approval::ordinary_calls_run_without_asking`（不出卡片）；`rg` / `ls` 与 `cat` 同属只读，未单独跑 |
| 3 | sandbox `matrix::auto_builds_and_reads_…`（四条 git 写命令被拒且工作区不变；`.git` 越界后提交成功，`.git/hooks` 仍拒）、`matrix::accept_edits_denies_bash_workspace_writes_until_escalated`（工作区授权仍不开 `.git`） |
| 4 | sandbox `matrix::accept_edits_denies_bash_workspace_writes_until_escalated`、`parity::bash_and_file_tools_differ_only_on_the_workspace_under_accept_edits`；tools `sandbox_calls::acc_10e_kernel_denials_are_marked_with_the_escalation_hint`；core `session_runtime::acc_38_44_…`（accept-edits 下 write 直接执行） |
| 5 | sandbox `matrix::protected_locations_stay_closed_and_temp_stays_open`（bash）、`parity::acc_10_file_tool_fence_and_seatbelt_agree_on_every_path`（两侧一致） |
| 6 | sandbox `parity::acc_10_…`；tools `skill_paths::read_can_open_an_agents_skill_outside_the_workspace` |
| 7 | tools `sandbox_calls::acc_10b_credential_directories_are_unreadable_for_file_tools_and_bash`、`read::rejects_read_through_symlink_into_a_credential_directory`；sandbox `matrix::protected_locations_…` |
| 8 | sandbox `parity::acc_10_…`、`parity::case_variants_of_protected_names_are_not_writable`、`policy::grants_unlock_what_they_name_but_never_hard_protected_paths` |
| 9 | sandbox `policy::grants_unlock_…`、`policy::a_broad_grant_does_not_open_protected_paths_it_merely_contains`、`policy::escalation_requests_are_validated`；tools `sandbox_calls::acc_10_an_escalation_widens_only_this_call_and_never_hard_protected_paths`、`skill_paths::write_and_edit_are_denied_for_the_agents_skill_root_in_every_mode`、`skill_paths::an_alias_cannot_bypass_…`；core `session_runtime::acc_09_39_…`、`approval::protected_write_targets_are_rule_denials_without_a_card`、`session_tools::acc_26_spilled_output_is_readable_and_never_writable` |
| 10 | sandbox `parity::acc_10_file_tool_fence_and_seatbelt_agree_on_every_path` |
| 11 | tools `bash::acc_11_bash_output_makes_no_network_or_isolation_claim`；`rg -i "network\|网络\|網路\|isolat"` 在三份文案、notice.rs、sandbox_policy.rs 中无命中 |
| 12 | sandbox `probe::acc_12_the_real_sandbox_passes_its_self_check`、`probe::a_sandbox_that_does_not_deny_fails_the_self_check` |
| 13 | tools `sandbox_calls::acc_10d_bash_does_not_run_when_the_sandbox_is_unavailable`、`prepare::an_unavailable_sandbox_reports_no_dangerous_command`；core `session_runtime::acc_13_an_unavailable_sandbox_never_asks_about_a_dangerous_command`、`acc_13_38_…`、`sandbox_policy::an_unavailable_sandbox_says_bash_is_unavailable`；desktop `SandboxUnavailableNotice.test`；无不经 Seatbelt 的 bash：完成条件 3 的 grep 为空 |
| 14 | tools `builtins::escalation_parameters_disappear_when_the_sandbox_is_unavailable` |
| 15 | sandbox `probe::acc_15_denials_and_sandbox_failures_are_told_apart`；tools `sandbox_calls::acc_10e_a_runner_failure_is_unavailable_not_denied` |
| 16 | 未自动化：只能在 Linux 上运行。代码检查：`probe.rs` 在非 macOS 上直接返回 `Unavailable`，与第 12 条的失败路径相同 |
| 17 | tools `sandbox_calls::acc_10e_kernel_denials_are_marked_with_the_escalation_hint`、`notice::bash_denial_names_the_mode_and_offers_escalation_only_when_available` |
| 18 | core `session_runtime::acc_18_19_…`、`approval::a_valid_escalation_asks_with_every_path_and_its_tier`；desktop `ApprovalDialog.test` |
| 19 | core `session_runtime::acc_18_19_…`（第二次同样请求再次出卡片，第三次调用的策略没有授权）；tools `sandbox_calls::acc_10_…` |
| 20 | 授权机制：sandbox `policy::grants_unlock_…`、`parity::acc_10_…`（被列出的路径可写，其他 `$HOME` 路径仍拒）；真实联网 `cargo build`：手动 |
| 21 | 凭据读授权：sandbox `parity::acc_10_…`；`.git/hooks` 任何越界下不可写：sandbox `matrix::auto_builds_…`；真实 `git push`：手动 |
| 22 | core `session_runtime::acc_22_…`（逐字：已可写、非绝对路径、理由为空；Turn 继续、无卡片）；sandbox `policy::escalation_requests_are_validated`（超过 16 条、硬保护、`/` 与 `$HOME` 子树） |
| 23 | 类型保证：`SandboxPermissionsInput` 只有 `paths`（`deny_unknown_fields`）；tools `builtins::builtin_registry_exposes_each_tool_once_in_selected_order`（顶层参数逐一断言，没有其他越界参数） |
| 24 | core `session_runtime::acc_24_a_user_denial_stops_the_turn_without_running_the_tool` |
| 25 | 结构保证：授权只存在于单次调用的 `SandboxPolicy`，会话状态与数据库都没有授权字段；core `session_runtime::acc_18_19_…`（批准后的下一次调用没有授权） |
| 26 | tools `danger::acc_26_27_28_29_the_listed_commands_hit_or_miss_as_specified`；core `session_runtime::acc_26_32_34_…`；desktop `ApprovalDialog.test`（高亮命中段与键） |
| 27–30 | tools `danger::acc_26_27_28_29_…`（逐条原样）、`danger::syntax_errors_are_not_checked`、`danger::nesting_beyond_the_limit_counts_as_a_hit` |
| 31 | core `approval::dangerous_commands_ask_only_in_auto`（批准不带授权）；sandbox `matrix::protected_locations_…`（`$HOME` 下写入被内核拒绝） |
| 32 | core `session_runtime::acc_26_32_34_…`、`approval::acc_32_an_escalation_card_also_marks_a_dangerous_command`；desktop `ApprovalDialog.test`（合并卡片） |
| 33 | core `approval::unattended_sessions_deny_what_would_need_a_card` |
| 34 | core `session_runtime::acc_26_32_34_…`（`dangerMatch`） |
| 35 | core `postgres_core_host_flow::acc_01_35_42_…`、`session_tools::a_sub_agent_never_gets_a_wider_mode_than_its_parent_or_role` |
| 36 | core `postgres_core_host_flow::acc_01_35_42_…`、`session_runtime::acc_36_37_…`；agent `builder::explorer_cannot_change_the_workspace`；内核行为：sandbox `matrix::accept_edits_denies_bash_workspace_writes_until_escalated` |
| 37 | core `session_runtime::acc_36_37_…`（越界直接拒绝、文本逐字）、`approval::unattended_sessions_deny_…`；目前没有上限为 `auto` 的子 Agent 角色，`auto` 子 Agent 跑 `cargo test` 由 `agent::builder::the_default_agent_may_use_the_widest_mode` 与第 2 条间接覆盖 |
| 38 | core `session_runtime::acc_13_38_…`、`acc_38_44_…`、`sandbox_policy::*` |
| 39 | core `session_runtime::acc_09_39_…`（逐字） |
| 40 | desktop `ApprovalDialog.test`（只有两个按钮）；core `updates::a_resolved_permission_carries_only_the_decision`；`PermissionDecision` 只有两个变体 |
| 41 | core `session_runtime::acc_41_multiple_permission_requests_are_presented_serially` |
| 42 | core `postgres_core_host_flow::acc_01_35_42_…`（重启后恢复）、`postgres_sandbox_mode_migration::existing_sub_agents_get_their_role_ceiling_and_roots_get_auto` |
| 43 | `rg -i "rules?\.(json\|toml)\|permission.*\.json\|allow_rules\|session_rules"` 在 crates 与 desktop/src 中无命中 |
| 44 | core `session_runtime::acc_38_44_…`、`acc_18_19_…`、`acc_26_32_34_…`、`acc_09_39_…`；desktop `permissionCategory.test`、`TraceTimeline.test` |
| 45 | `SandboxMode` 只有两个变体；`record_sandbox_mode` 只接受 `SandboxMode` |
| 46 | desktop `TurnTraceDrawer.test`（`data-trace-completeness="partial"`，本改动前已有） |
| 47 | 完成条件 4 的 grep 为空；`openwork-tools/src/permission/` 只剩 `danger.rs` 与 `mod.rs` |
| 48 | `rg -l tree_sitter crates` 只命中 `openwork-tools/src/permission/danger.rs` |
| 49 | sandbox `matrix::a_home_workspace_is_read_only_for_bash`、`policy::a_workspace_containing_home_is_read_only_for_bash_in_every_mode` |
| 50 | sandbox `matrix::go_builds_and_tests_with_the_private_cache`、`policy::bash_environment_points_the_go_cache_at_a_private_temp_directory` |

### tools.md §12 #7–10f

| # | 证据 |
|---|---|
| 7 | tools `write::rejects_new_file_through_symlink_outside_workspace`、`write::rejects_protected_metadata_through_symlink_alias`、`read::rejects_read_through_symlink_into_a_credential_directory`、`read::allows_read_through_symlink_that_stays_inside_workspace` |
| 8 | tools `write::rejects_new_file_through_symlink_outside_workspace`、`write::rejects_new_file_through_dangling_symlink`；sandbox `parity::acc_10_…`（不存在的新路径） |
| 9 | 结构保证：`CheckedPath` 字段私有，只有 `resolve_path` 能构造 |
| 10 | tools `sandbox_calls::acc_10_an_escalation_widens_only_this_call_and_never_hard_protected_paths` |
| 10a | sandbox `parity::acc_10_…` |
| 10b | tools `sandbox_calls::acc_10b_…` |
| 10c | tools `sandbox_calls::acc_10c_bash_runs_under_the_policy_of_this_call` |
| 10d | tools `sandbox_calls::acc_10d_…`；完成条件 3 的 grep 为空 |
| 10e | tools `sandbox_calls::acc_10e_kernel_denials_…`、`acc_10e_a_runner_failure_is_unavailable_not_denied` |
| 10f | tools `builtins::escalation_parameters_disappear_when_the_sandbox_is_unavailable` |

### multi-agent.md §11 #15–16

| # | 证据 |
|---|---|
| 15 | 内核拒绝写 `target/`：sandbox `matrix::accept_edits_denies_bash_workspace_writes_until_escalated`；拒绝标记：tools `sandbox_calls::acc_10e_…`；越界重试直接拒绝、文本可操作：core `session_runtime::acc_36_37_…` |
| 16 | core `session_runtime::acc_36_37_…`（`git log` 执行、无审批卡片）、`postgres_core_host_flow::acc_01_35_42_…`（父会话 `auto` 时 explorer 为 `accept_edits`） |
