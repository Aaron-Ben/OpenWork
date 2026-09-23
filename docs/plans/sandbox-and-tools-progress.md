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
| WP3 沙箱切换 | 未开始 | 依赖 WP2 |
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

## 3. 待定项

| # | 事项 | 现状与建议 |
|---|---|---|
| — | 工具改进（对照 ZCode 等得出，暂不做） | read 重复读取去重；grep 默认只列文件并支持 `-A/-B/-C`；按时间或空闲修剪旧结果；edit 行号前缀与弯引号容错（并入 WP4）；超限预览缩小；先看 Trace 缓存命中数据再决定 |
| — | multi-agent.md 过时段落（与本计划无关，只记录） | 说 `last_real_user`"必须改"、"现在"会选中子 Agent 消息，并链接到已不存在的 `compaction/projection.rs`；代码已在 `compaction/compacted_view.rs:26` 按 kind 排除 contextual 消息。需要改写该段与链接 |
| — | tools.md 出处错误 | "bash 保留头 2 KB + 尾 14 KB 来自 DSH"不对，DSH 只保留尾部。WP7 时改 |

## 4. 参照任务回放

对 `0b44333` 的冻结快照按固定顺序执行同一组工具调用；token 按 4 字节 / token 估算，"投影后"指经请求投影单条 8000 token 上限后的值。

| 时点 | 原始 token | 投影后 token |
|---|---|---|
| WP0 基线 | 84,650 | 42,927 |
| WP1 之后 | 41,251 | 41,251（−4%） |

WP1 的收益主要是正确性：不再从中间截断、总数准确、完整结果可取回。WP4 后重跑并补一行。
