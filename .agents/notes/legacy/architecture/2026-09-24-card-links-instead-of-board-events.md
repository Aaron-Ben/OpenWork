# Agent Note: 房间与看板之间只用卡片链接连接

Status: legacy

## 问题

讨论发生在房间里，工作记在看板上。用户和 Agent 都需要从一条消息找到它说的那张卡片，也需要从卡片回到讨论。

房间与看板之间要有某种连接。连接的方式决定两件事：Server 要不要多一条写入路径，以及房间历史里会不会混进看板变化。

## 决策

房间与看板之间唯一的连接是消息正文里的卡片 id。Server 不把看板事件写进房间。

- `AGENTS.md` 契约要求 Agent 谈到卡片时写出它的 id：`When you talk about a Board card, write its id (card-…) so the room can link to it.`（`crates/openwork-collab/src/computer/home.rs`）。
- Desktop 在 `desktop/src/features/collab/rooms/messageText.ts` 中识别 `card-[0-9a-f]{32}`，代码块与行内代码中的 id 不算（`cardIdsOutsideCode`）。
- `rooms/CardLinks.tsx` 的 `CardChip` 把 id 渲染成胶囊，`CardSummary` 在消息下方附摘要卡。
- 点击后，`rooms/CardPreviewPanel.tsx` 在房间右侧栏显示卡片预览。“打开看板”跳到看板页并选中这张卡片。
- 卡片详情的“在房间中讨论”打开与负责人的私聊，并预填卡片引用。

做法来自 Cumora：`src/components/CardLink.tsx` 渲染卡片链接。Cumora 的 `enqueueBoardCli`（`server/src/agents/cli.ts`）只在看板频道广播 `board.changed`，不写会话消息。

规则见 [collaboration.md](../../../../docs/subsystems/collaboration.md) §7.1，界面见 [collaboration-desktop.md](../../../../docs/subsystems/collaboration-desktop.md) §7.2、§7.5 与 §9。

## 考虑过的方案

**看板事件自动写入房间。** 卡片创建、移动或改派时，Server 往相关房间写一条消息。当时明确决定不做，理由是按 Cumora 的方式连接。原始记录没有写其他理由。

## 后果

- Server 不新增写入路径。看板变化不产生消息，所以不改变消息唤醒、triage、lap floor 与未读数。
- 连接依赖模型自觉写出 id。模型不写，房间里就没有链接。契约里只有一句提示。
- 房间历史里看不到“谁在什么时候移动了卡片”。看板只显示卡片的当前状态。
- 卡片已删除时，胶囊只显示 id，且不可点击。
- 胶囊标题从已加载的 Board 快照中查找。所以 board invalidation 在房间页也要重新读取 Board（collaboration-desktop.md §5）。
- 测试：`messageText.test.ts`（代码中的 id 不算）、`roomPage.render.test.tsx`（胶囊、已删除卡片与摘要卡）、`computer::home::tests::acc_08_standing_contract_names_the_addressing_rules`。
