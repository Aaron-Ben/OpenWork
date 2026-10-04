# Agent Note: Desktop 用户直接建卡、编辑与移动卡片

Status: legacy

## 问题

看板是用户与 Agent 共享的工作记录。用户想交代一件事、修正一个标题或把卡片挪到另一列时，如果只能在房间里请 Agent 去做，每次都要多一轮对话，还要多跑一次模型。

## 决策

- Desktop 命令 `CreateCard`、`UpdateCard`、`MoveCard`、`AssignCard` 与 `DeleteCard` 由 `crates/openwork-collab/src/server/desktop_cards.rs` 处理。
- 新建时指定负责人、改派与新增 `@<agent-id>` 都写入卡片唤醒，发起者是 `local-user`，不受每分钟 30 次的限额。结果 `CardChangeView` 带回被唤醒的 Agent。
- Column 与 Board 的结构仍只归 Desktop 用户。

做法来自 Cumora：`src/desktop/BoardsView.tsx` 让人在看板上建卡（`addCard`）与拖动卡片。

规则见 [collaboration.md §11.2](../../../../docs/subsystems/collaboration.md)，界面见 [collaboration-desktop.md](../../../../docs/subsystems/collaboration-desktop.md)。

## 考虑过的方案

**Desktop 只分配与删除卡片，其余由 Agent 处理。** 这是此前的规则。之后对照 Cumora 看板，改为用户也能建卡、编辑与移动。原始记录没有写更多理由。

## 后果

- 用户不必经过 Agent，就能维护看板。
- 用户编辑卡片同样会唤醒 Agent。用户在描述里写 `@<agent-id>`，就等于把工作交给它。
- 用户与 Agent 可能同时改同一张卡片。Server 按 Board → Column → Card 的顺序加锁，后提交的覆盖先提交的。
- 测试：`tests/desktop_cards.rs::acc_14_desktop_creates_edits_and_moves_cards_and_wakes_who_it_names`。
