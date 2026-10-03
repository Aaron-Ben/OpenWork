# Agent Note: 运行记录页保留在普通导航

Status: implemented

## 问题

协作界面要回答两类问题。第一类是“现在怎样”：谁在工作，谁出错了，哪张卡片在等谁。第二类是“某次运行做了什么”：用了多久，在哪一步失败。

房间、Agent 与看板页面回答第一类。第二类需要 Run 级别的细节。把这些细节放进房间，聊天界面会变成日志。完全不提供，用户在 Agent 出错或不回复时就无从查起。

这个页面放在哪里、给谁看，也要定下来。

## 决策

- “运行记录”是 `CollabRail` 的第五个顶层目的地（`desktop/src/features/collab/components/CollabRail.tsx`，view 为 `observability`）。它在普通导航里，不设开发者模式（用户确认，2026-09-25）。
- 页面在 `desktop/src/features/collab/observability/`：`ObservabilityPage.tsx`、`TraceDetail.tsx` 与 `observabilityStore.ts`。左侧的 Run 列表可以按 Agent 与状态筛选，右侧显示选中 Run 的事件时间线。页面默认每 3 秒刷新一次，用户可以关闭。
- Tauri 命令是 `collab_run_list` 与 `collab_run_trace`（`desktop/src-tauri/src/commands/collab.rs`）。Server 侧是 `crates/openwork-collab/src/server/observability.rs` 的 `Observability::list_runs` 与 `trace`。
- 时间线合并四个来源：`collab_run_events`（迁移 `202609010001_run_observability.sql`）、`collab_triages`、`collab_command_requests`，以及 Run 的开始与结束。
- 房间里不展示运行细节，只用说明行解释“为什么有人没回复”。

设计见 [collaboration-desktop.md](../../../../docs/subsystems/collaboration-desktop.md) §1、§4.4 与 §10，存储见 [collaboration.md](../../../../docs/subsystems/collaboration.md) §13.3.7。

## 考虑过的方案

**删除运行记录。** 2026-09-24 曾定下：协作模式只展示当前状态与说明行，删除运行记录页与它的专用后端。2026-09-25 出现新事实：Cumora 有观测页（`src/desktop/ObservabilityView.tsx`），其中包括运行记录与事件时间线。用户决定保留，撤销了删除的决定。删除的决定没有进入代码，运行记录的代码一直保留。

**只在开发者模式中显示。** Cumora 把观测页藏在开发者模式后面：`server/src/api/router.ts` 的 `getDevtoolsState` 只允许 owner 与 admin 开启，本地开发构建默认开启。`src/desktop/DesktopApp.tsx` 只在 `devtoolsEnabled` 时渲染这个页面。没有采用：OpenWork 是单用户本机应用，没有角色之分，开发者模式只会多一个开关。

## 后果

- 用户不用切换模式，就能查 Agent 为什么失败、为什么没回复。
- 代价：普通用户也会看到偏技术的页面，导航多一项。
- 运行记录只用于观察，不提供重试、取消或编辑。结算、路由与一轮上限的判定都不读 `collab_run_events`。
- Cumora 观测页另有三个面板：Agent 工作区、triage 经济性与唤醒经济性（`ObservabilityView.tsx` 的 `DEV_PANELS`）。决定保留运行记录时，是否补齐它们列为待定。之后的界面决定没有单独处理这一项。当前代码没有这三个面板。
- Runner 退避不在 Agent 页显示，要查原因就看这里失败的 Run。见 [不上报 Runner 暂停](../simplification/2026-09-25-no-runner-pause-reporting.md)。
- 测试：`observabilityStore.test.ts`，以及 `bridge/collab.test.ts` 中 `collab_run_list` 与 `collab_run_trace` 的用例。
