import type {
  CollabBoard,
  CollabBoardColumn,
  CollabCard,
  CollabRoomMessage,
  CollabRoomNote,
} from '@/bridge/collab'

export type TimelineItem =
  | { key: string, kind: 'message', message: CollabRoomMessage }
  | { key: string, kind: 'note', note: CollabRoomNote }

/** 消息按 sequence 排列，说明行紧跟在它解释的那条消息后面（collaboration-desktop.md §7.3）。 */
export function roomTimeline(messages: CollabRoomMessage[], notes: CollabRoomNote[]): TimelineItem[] {
  return messages.flatMap((message): TimelineItem[] => [
    { key: message.id, kind: 'message', message },
    ...notes
      .filter((note) => note.afterSequence === message.sequence)
      .map((note): TimelineItem => ({ key: `note-${note.kind}-${note.afterSequence}`, kind: 'note', note })),
  ])
}

export interface FoundCard {
  card: CollabCard
  board: CollabBoard
  column: CollabBoardColumn
}

/** 在已加载的看板里找卡片；卡片已删除或看板还没加载时返回 `null`。 */
export function findCard(boards: CollabBoard[], cardId: string): FoundCard | null {
  for (const board of boards) {
    for (const column of board.columns) {
      const card = column.cards.find((candidate) => candidate.id === cardId)
      if (card) return { card, board, column }
    }
  }
  return null
}
