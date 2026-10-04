# Agent Note: 派生配置把 Agent 的模型标为 active

Status: legacy

## 问题

每个 RuntimeSession 的 `XDG_CACHE_HOME` 都是一个空目录（`runtime/<session-id>/derived/<id>/<engine>/cache`）。OpenCode 先用自带的模型目录快照，运行时再刷新模型目录。刷新后的目录把 Desktop 默认模型 `deepseek/deepseek-v4-flash` 标为 deprecated。OpenCode 会删除 deprecated 的模型（opencode `packages/opencode/src/provider/provider.ts`）。

结果是第一轮成功，第二轮起全部失败（`Unexpected server error`）。DeepSeek API 仍能调用这个模型，失败只来自 OpenCode 本地的判断。

## 决策

- `prepare_environment`（`crates/openwork-collab/src/computer/opencode/launch.rs`）写派生配置时，加入 `provider.<p>.models.<m>.status = "active"`。模型 id 按第一个 `/` 拆成服务商与模型名。
- `turn_environment` 为正式 Turn 写主模型。`classify_environment` 在 `classify/` 配置目录写 triage 模型。
- OpenCode 合并配置时，配置里的 `status` 优先于目录里的状态。目录里没有这个模型时，这个条目也会创建它（`provider.ts` 中 `model.status ?? existingModel?.status ?? "active"`）。
- Desktop 默认模型改为 `deepseek/deepseek-flash`（`desktop/src/features/collab/agents/AgentFormDialog.tsx` 的 `DEFAULT_MODEL`）。

规则见 [collaboration.md §6](../../../../docs/subsystems/collaboration.md)。

## 考虑过的方案

**持久化缓存目录。** 这次修正的决定写明：缓存目录仍是每次会话的临时目录，持久化另议。它不是这次的修法。按 `provider.ts`，删除 deprecated 模型只看目录里的状态。所以持久化的缓存仍会让模型失效。

**预置新的模型目录。** 此前第四次点名实测预置了新目录，让 `deepseek/deepseek-flash` 能运行。这只是测量时的临时手段。这次修正后的复测特意不预置目录，以证明派生配置本身足够。

## 后果

- 用户选的模型不会因目录刷新而失效。验收 `opencode_adapter::derived_configs_keep_the_chosen_models_active_even_when_the_catalog_deprecates_them`。
- 实测：不预置目录时，`deepseek/deepseek-v4-flash` 两轮都成功。运行后缓存里的 `models.json` 把它标为 deprecated，派生配置里是 active。
- OpenWork 不再借 OpenCode 的 deprecated 标记拦住旧模型。服务商真正下线模型时，错误来自服务商 API。
- 目录里没有的模型，除 `status` 外的字段取 OpenCode 的默认值。
- 缓存仍在每个会话清空。每个 RuntimeSession 第一次运行时，OpenCode 都会重新刷新目录。
