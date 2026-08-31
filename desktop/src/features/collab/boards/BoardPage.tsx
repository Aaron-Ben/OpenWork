import { ClipboardList, Plus } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabBoard, CollabBoardColumn } from '@/bridge/collab'
import { Button } from '@/components/ui/button'
import { useAgentStore } from '@/features/collab/agents/agentStore'
import { ResizableSidebarLayout } from '@/features/collab/components/ResizableSidebarLayout'
import { BoardCanvas } from './BoardCanvas'
import {
  BoardEditorDialog,
  ColumnEditorDialog,
  DeleteBoardEntityDialog,
  type DeletableBoardEntity,
} from './BoardDialogs'
import { BoardSidebar } from './BoardSidebar'
import { useBoardStore } from './boardStore'

type BoardDialog =
  | { kind: 'board'; board: CollabBoard | null }
  | { kind: 'column'; boardId: string; column: CollabBoardColumn | null }
  | { kind: 'delete'; entity: DeletableBoardEntity; id: string; label: string }

export function BoardPage() {
  const { t } = useTranslation()
  const boards = useBoardStore((state) => state.boards)
  const agents = useAgentStore((state) => state.agents)
  const selectedBoardId = useBoardStore((state) => state.selectedBoardId)
  const loading = useBoardStore((state) => state.loading)
  const error = useBoardStore((state) => state.error)
  const fetchAll = useBoardStore((state) => state.fetchAll)
  const [dialog, setDialog] = useState<BoardDialog | null>(null)
  const selectedBoard = boards.find((board) => board.id === selectedBoardId) ?? null

  useEffect(() => {
    void fetchAll()
  }, [fetchAll])

  return (
    <section className="flex min-w-0 flex-1 flex-col overflow-hidden bg-paper">
      <header data-tauri-drag-region="deep" className="flex h-12 shrink-0 items-center justify-between border-b border-line px-6">
        <h1 className="font-serif text-lg font-semibold">{t('collab.boards.title')}</h1>
        <Button type="button" size="sm" onClick={() => setDialog({ kind: 'board', board: null })}>
          <Plus size={15} />{t('collab.boards.create')}
        </Button>
      </header>
      <div className="relative min-h-0 flex-1">
        {error ? (
          <p className="absolute left-1/2 top-3 z-30 max-w-xl -translate-x-1/2 rounded-xl border border-status-danger-border bg-paper px-3 py-2 text-sm text-status-danger-ink shadow-sm">
            {error}
          </p>
        ) : null}
        <ResizableSidebarLayout
          className="h-full"
          storageKey="boards"
          defaultWidth={272}
          minWidth={220}
          maxWidth={440}
          resizeLabel={t('collab.boards.resizeSidebar')}
          sidebar={<BoardSidebar />}
        >
          {selectedBoard ? (
            <BoardCanvas
              board={selectedBoard}
              agents={agents}
              onEditBoard={(board) => setDialog({ kind: 'board', board })}
              onAddColumn={(boardId) => setDialog({ kind: 'column', boardId, column: null })}
              onEditColumn={(boardId, column) => setDialog({ kind: 'column', boardId, column })}
              onDelete={(entity, id, label) => setDialog({ kind: 'delete', entity, id, label })}
            />
          ) : (
            <div className="grid h-full place-items-center text-ink-faint">
              <div className="text-center">
                <ClipboardList className="mx-auto mb-3 opacity-50" size={36} />
                <p className="text-sm">{loading ? t('common.loading') : t('collab.boards.empty')}</p>
              </div>
            </div>
          )}
        </ResizableSidebarLayout>
      </div>

      {dialog?.kind === 'board' ? <BoardEditorDialog board={dialog.board} onClose={() => setDialog(null)} /> : null}
      {dialog?.kind === 'column' ? (
        <ColumnEditorDialog boardId={dialog.boardId} column={dialog.column} onClose={() => setDialog(null)} />
      ) : null}
      {dialog?.kind === 'delete' ? (
        <DeleteBoardEntityDialog
          kind={dialog.entity}
          id={dialog.id}
          label={dialog.label}
          onClose={() => setDialog(null)}
        />
      ) : null}
    </section>
  )
}
