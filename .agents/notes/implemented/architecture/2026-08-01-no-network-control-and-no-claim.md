# Agent Note: 不管控网络，也不声称管控

Status: implemented

## 问题

沙箱内的命令可以联网。理论上，它们能把工作区内能读到的内容发出去。系统要么管控网络，要么在某个地方说明这一点。

管控网络需要代理、白名单与失败提示，是独立的一块工作。只做说明也有问题：一个永远显示“未强制”的声明，用户会习惯忽略它。模型也会被这类声明影响。

## 决策

- Seatbelt profile 只管文件效果，不含网络规则（`crates/openwork-sandbox/src/seatbelt.rs` 的 `SeatbeltProfile::new`）。`(allow default)` 之后只有 `file-write*` 与 `file-read*` 规则。
- 界面、卡片、工具结果与 `runtime/sandbox-policy` section 都不提网络限制或隔离。`sandboxPermissions` 只有 `paths`，没有网络字段（`crates/openwork-tools/src/escalation.rs`）。
- 凭据禁读档（`crates/openwork-sandbox/src/tiers.rs` 的 `CREDENTIAL_PATHS`）把最值钱的数据从“能读到”里拿掉。它是文件规则，不是网络规则。

设计见 [permissions.md](../../../../docs/subsystems/permissions.md)。

## 考虑过的方案

**由执行层做网络出口限制。** 2026-07-11 的架构规划把“网络出口限制”列为执行层的职责，与 macOS 沙箱一起做。2026-08-01 的设计放弃了它：当时不做沙箱，没有隔离手段，就没有可强制的网络边界。2026-09-24 改用沙箱后，仍然暂不接入。

**默认断网加代理白名单。** Codex 的 Seatbelt profile 接入了网络代理（`codex-rs/sandboxing/src/seatbelt.rs` 引用 `codex_network_proxy`，代理在 `codex-rs/network-proxy/`）。默认断网与白名单的具体行为没有读源码（未确认）。没有采用：现在不接入。

**不管控，但在界面上声明网络未隔离。** 没有采用：恒为“未强制”的声明只会训练用户忽略它。2026-08-01 的设计已有这条结论。

## 后果

- 代价：沙箱内的命令可以把工作区内容与非凭据文件发到网络上。这是公开写明的风险。
- 网络比 Codex 的默认档更松。
- 读取默认放开（allow default）依赖这个前提。接入网络管控时，要重新评估默认拒绝，见 [路径分四档，凭据目录禁读](2026-08-01-path-tiers-and-credential-read-deny.md)。
- 模型看到的策略只陈述事实，不提网络，见 [沙箱策略作为最后一个 world state section](2026-09-24-sandbox-policy-world-state-section.md)。
- 测试：`crates/openwork-tools/src/builtins/process/bash.rs` 的 `acc_11_bash_output_makes_no_network_or_isolation_claim` 只检查 bash 的输出。界面文案、卡片与策略 section 没有同类的自动测试。
