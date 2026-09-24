import type { CollabBoard, CollabCard } from '@/bridge/collab'

/** 标题栏的“N 张卡片 · N 位 Agent 在做”（collaboration-desktop.md §9）。 */
export function boardSummary(board: CollabBoard): { cardCount: number, workingAgentCount: number } {
  const cards = board.columns.flatMap((column) => column.cards)
  const working = new Set(cards
    .filter((card) => card.agentState === 'working' && card.assigneeId)
    .map((card) => card.assigneeId))
  return { cardCount: cards.length, workingAgentCount: working.size }
}

/**
 * 拖到目标列第 `dropIndex` 个位置（按界面上显示的卡片计，含被拖的那张）时，放在哪张卡片前面；
 * 放到末尾时为 `null`。Server 据此重排，界面不自己算 position（collaboration.md §11.2）。
 */
export function beforeCardIdAt(cards: CollabCard[], draggedId: string, dropIndex: number): string | null {
  return cards.slice(dropIndex).find((card) => card.id !== draggedId)?.id ?? null
}

/** 放下后位置不变（同一列、前后相邻的卡片没变）时不必发命令。 */
export function isNoopMove(
  columnCards: CollabCard[],
  dragged: CollabCard,
  columnId: string,
  beforeCardId: string | null,
): boolean {
  if (dragged.columnId !== columnId) return false
  const index = columnCards.findIndex((card) => card.id === dragged.id)
  return (columnCards[index + 1]?.id ?? null) === beforeCardId
}

/** 卡片底部显示什么：处理中、已唤醒排队、未分配，或最近更新时间。 */
export type CardFooter = 'working' | 'queued' | 'unassigned' | 'updated'

export function cardFooter(card: CollabCard): CardFooter {
  if (card.agentState === 'working') return 'working'
  if (card.agentState === 'queued') return 'queued'
  if (!card.assigneeId) return 'unassigned'
  return 'updated'
}

/** 拖动时鼠标在第几个位置：`midpoints` 是列里各卡片纵向中点，鼠标在几个中点之下就是第几个位置。 */
export function dropIndexAt(midpoints: number[], pointerY: number): number {
  return midpoints.filter((midpoint) => midpoint < pointerY).length
}
