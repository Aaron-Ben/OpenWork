# Agent Note: 可写设备清单，不单列 /dev/stdout 与 /dev/stderr

Status: implemented

## 问题

Seatbelt profile 先 `(deny file-write*)`，再逐条放行。脚本常按路径写标准流：`echo x > /dev/stdout`、`tee /dev/stderr`、`> /dev/fd/3`，以及进程替换 `tee >(cat)`。如果只放行 `/dev/null`，这些写法在开发机上都失败。模型会把这种失败读成命令有缺陷，然后换写法重试。

草案的清单单列了 `/dev/stdout` 与 `/dev/stderr`。它们是指向 `/dev/fd/1`、`/dev/fd/2` 的符号链接。Seatbelt 按解析后的路径判断，所以单列它们不起作用。

## 决策

- `crates/openwork-sandbox/src/tiers.rs` 的 `WRITABLE_DEVICES` 是 `/dev/null`、`/dev/zero`、`/dev/tty`。`WRITABLE_DEVICE_REGEXES` 覆盖 `/dev/fd/<n>` 与 `/dev/ttys<n>`。
- `is_writable_device` 给文件工具围栏用。`seatbelt.rs` 用同一组常量生成 profile 规则。
- 这些设备在两个模式下都可写。`/dev` 下仍不能创建文件。
- 测试：`matrix::stream_devices_are_writable_in_every_mode`。五种写法在两个模式下都成功，`touch /dev/openwork-matrix-probe` 被拒。另有单元测试 `tiers::numbered_devices_are_writable_but_other_dev_paths_are_not`。

公开接口见 [openwork-sandbox README](../../../../crates/openwork-sandbox/README.md)。

## 考虑过的方案

**只放行 `/dev/null`。** 这是 Codex 默认档的做法。`codex-rs/sandboxing/src/seatbelt_base_policy.sbpl` 只对 `/dev/null` 放行 `file-write-data`。`/dev/fd/(1|2)` 与 `/dev/tty` 写在 `seatbelt_read_only_platform_defaults.sbpl` 里。`codex-rs/sandboxing/src/seatbelt.rs` 只在 `include_platform_defaults()` 为真时加入这一段。按 `codex-rs/protocol/src/permissions.rs`，这要求受限读取且带 Minimal 平台条目。没有采用：上面那些常见写法会失败。原始记录说这些写法在 Codex 里同样失败。这一点只读过源码，没有在 Codex 中运行验证（未确认）。

**在清单里单列 `/dev/stdout` 与 `/dev/stderr`。** 草案这样写。没有采用：Seatbelt 按解析后的路径判断，这两条永远不会命中。留着它们，读者会以为放行靠的是它们。

## 后果

- 按路径写标准流与额外的文件描述符都能用。
- 放行范围比 Codex 默认档宽：`/dev/zero`、`/dev/tty` 与任意编号的 `/dev/fd/<n>` 都可写。
- `>&3` 这类复制描述符的写法不经过路径，本来就不受影响。
- 围栏同样只认 `/dev/fd/<n>`，不认 `/dev/stdout`。它依赖调用方先把路径规范化。
