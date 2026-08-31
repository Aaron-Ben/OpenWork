import { CheckCircle2, ClipboardList } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { useBoardStore } from './boardStore'

export function BoardSidebar() {
  const { t } = useTranslation()
  const boards = useBoardStore((state) => state.boards)
  const selectedBoardId = useBoardStore((state) => state.selectedBoardId)
  const selectBoard = useBoardStore((state) => state.selectBoard)

  return (
    <aside className="flex h-full w-full flex-col overflow-hidden border-r border-line bg-paper-hover">
      <div className="border-b border-line px-4 py-3 text-xs font-semibold uppercase tracking-[0.12em] text-ink-faint">
        {t('collab.boards.allBoards')}
      </div>
      <nav className="min-h-0 flex-1 overflow-y-auto p-2">
        {boards.map((board) => {
          const cardCount = board.columns.reduce((count, column) => count + column.cards.length, 0)
          const completeCount = board.columns
            .filter((column) => column.isTerminal)
            .reduce((count, column) => count + column.cards.length, 0)
          return (
            <button
              key={board.id}
              type="button"
              className={`mb-1 grid w-full gap-1 rounded-xl px-3 py-2.5 text-left transition ${selectedBoardId === board.id ? 'bg-paper shadow-sm ring-1 ring-line' : 'hover:bg-paper/70'}`}
              onClick={() => selectBoard(board.id)}
            >
              <span className="flex min-w-0 items-center gap-2">
                <ClipboardList className="shrink-0 text-clay" size={15} />
                <strong className="truncate text-sm">{board.title}</strong>
              </span>
              <span className="flex items-center justify-between gap-2 pl-6 text-[11px] text-ink-faint">
                <span>{t('collab.boards.cardCount', { count: cardCount })}</span>
                {completeCount > 0 ? (
                  <span className="inline-flex items-center gap-1 text-status-success-ink">
                    <CheckCircle2 size={11} />{completeCount}
                  </span>
                ) : null}
              </span>
            </button>
          )
        })}
        {boards.length === 0 ? (
          <p className="px-3 py-10 text-center text-sm text-ink-faint">{t('collab.boards.empty')}</p>
        ) : null}
      </nav>
    </aside>
  )
}
