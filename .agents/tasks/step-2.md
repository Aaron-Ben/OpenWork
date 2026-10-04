# 第 2 步：最小私聊

范围见[设计 Agent Note](../notes/proposed/architecture/2026-10-04-typescript-rewrite.md)的“第 2 步的实现决策”。全部完成后删除本文件。

## 已完成

- [x] 第 1 块：数据库表结构、迁移与本机用户
- [x] 第 2 块：Server 接口（Agent、消息、inbox、Agent 凭证、SSE）
- [x] 第 3 块：共用的 SSE 读取与重连
- [x] 第 4 块：Computer（沙箱、Agent 目录、OpenCode 适配器、AgentRunner、主流程）
- [x] 第 5 块：`crew` 命令
- [x] 第 6 块：私聊界面，目录调整为 `electron/` 与 `src/`

## 第 7 块：收尾

- [x] 测试流程：冒烟测试、真实模型 e2e、`testing.md`、`defensive-patterns.md`、提交流程 skill、本次规则调整。
  完成标准：`pnpm check` 通过；`CREW_E2E_MODEL=<模型> pnpm test:e2e` 通过；已提交。
- [ ] 检查脚本：文档路径存在、Biome 的 `noConsole` 与 `noEmptyBlockStatements`、禁止 `as unknown`、commit-msg 钩子。
  完成标准：每项都有测试证明它拒绝违规输入；`pnpm check` 通过；已提交。
- [ ] 界面截图命令 `pnpm preview:shot`：用临时数据库启动应用并截图，结束时清理。
  完成标准：截图能显示空状态；没有残留进程与临时库。
- [ ] 文档：`docs/architecture.md`、`docs/subsystems/messaging.md`、`docs/subsystems/agent-runtime.md`、根目录 `README.md`。
  完成标准：路径检查通过；文档索引已更新。
- [ ] 对照 DSH 的 defensive-patterns 复查子进程与清理代码（交给 subagent，核对每条证据）。
  完成标准：问题已修复或记录在汇报中。
- [ ] 逐条确认 Note 的验收条件。
  完成标准：每条写出证据（测试名或命令输出）。
