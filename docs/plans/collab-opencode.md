# 开发计划：协作 OpenCode 接入修正

**这是一份执行计划，不是功能文档。** 设计只以 [collaboration.md](../collaboration.md) 与 [permissions.md](../permissions.md) 为准。本文只说明按什么顺序做、怎样算完成。全部工作包完成后，删除本文。

## 1. 来源

本计划对照 Cumora 本地模式（BYOA）已提交的源码 `/Volumes/Extreme SSD/Code/cumora/server/src/agents/computer/`。目标是找出 `openwork-collab` 在"本机、只接 OpenCode、不做移动端"范围内的缺口。有些能力 Cumora 有，但 raw.md 已决定不做，例如 Memory、Skills、reaction、calendar、steer。这些能力不在本计划内。

## 2. 工作包

| WP | 内容 | 依据 | 完成条件 |
|---|---|---|---|
| C1 | 协作契约进入 OpenCode：正式 Turn 的派生配置用 `instructions` 引用受管 `AGENTS.md`；classify 使用独立配置目录 | opencode `session/instruction.ts`：`OPENCODE_DISABLE_PROJECT_CONFIG` 下不读项目 AGENTS.md；`config/config.ts` 拼接各层 `instructions` | `main_turn_config_loads_the_managed_agents_file_as_instructions`、`classify_config_does_not_load_agent_instructions` |
| C2 | Engine 失败需要用户处理时（未登录、凭证无效、额度不足），暂停该 Agent 15 分钟；聊天与 Agenda 两条路径共用这一规则 | Cumora `daemon.ts` `classifyTurnOutcome` / `ENGINE_BACKOFF_AFTER_OPERATOR_FIX_MS` | 失败后不再每次轮询都重新拉起 Engine；成功后解除暂停 |
| C3 | OpenCode 进 Seatbelt（D1 = B）：只限制文件范围，不限制网络 | permissions.md、`openwork-sandbox` | 待设计。新增 `openwork-collab → openwork-sandbox` 依赖前，先改 architecture.md |
| C4 | 上下文超长或坏字符导致失败时，重置 Engine session | Cumora `daemon.ts` `mustResetSession` | 待定 |
| C5 | inventory 上报 `opencode --version` | Cumora `cli-version.ts` | 待定 |
| C6 | Desktop 从 `opencode models` 选择模型 | Cumora `model-catalog.ts` | 待定，涉及 protocol 与 `compat.ts` |
| C7 | 并入 [collab-core.md](collab-core.md) 的 K1 | — | — |
