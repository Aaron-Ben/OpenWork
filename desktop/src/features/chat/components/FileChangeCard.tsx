import { useState } from 'react'
import { Check, ChevronDown, Eye, FilePenLine, Redo2, RotateCcw } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { resolveErrorMessage } from '@/lib/commandError'
import { FileStats, type FileChangeView } from './FileDiffPanel'

interface FileChangeCardProps {
  changes: FileChangeView[]
  onUndoFileChanges?: (changeIds: string[]) => Promise<void>
  onReapplyFileChanges?: (changeIds: string[]) => Promise<void>
  /// 审阅面板是页面右栏，不归卡片管，卡片只负责把这批改动递上去。
  onReviewFileChanges?: (changes: FileChangeView[]) => void
}

export function FileChangeCard({
  changes,
  onUndoFileChanges,
  onReapplyFileChanges,
  onReviewFileChanges,
}: FileChangeCardProps) {
  const { t } = useTranslation()
  const [showAll, setShowAll] = useState(false)
  const [operation, setOperation] = useState<'undo' | 'reapply' | null>(null)
  const [operationError, setOperationError] = useState<{
    kind: 'undo' | 'reapply'
    message: string
  } | null>(null)
  const additions = changes.reduce((total, change) => total + change.additions, 0)
  const deletions = changes.reduce((total, change) => total + change.deletions, 0)
  const someUndone = changes.some((change) => change.undone)
  const allUndone = changes.length > 0 && changes.every((change) => change.undone)
  const mixedState = someUndone && !allUndone
  const visibleChanges = showAll ? changes : changes.slice(0, 3)
  const hiddenCount = changes.length - visibleChanges.length
  const title = changes.length === 1
    ? changes[0].kind === 'created'
      ? t('tool.createdFile')
      : t('tool.editedOneFile')
    : t('tool.editedFileCount', { count: changes.length })

  async function applyFileChangeAction() {
    if (operation || mixedState) return
    const kind = allUndone ? 'reapply' : 'undo'
    const action = allUndone ? onReapplyFileChanges : onUndoFileChanges
    if (!action) return
    setOperation(kind)
    setOperationError(null)
    try {
      await action(changes.map((change) => change.changeId))
    } catch (error) {
      setOperationError({ kind, message: resolveErrorMessage(error) })
    } finally {
      setOperation(null)
    }
  }

  return (
    <div
      data-file-change-summary="true"
      className="overflow-hidden rounded-2xl border border-line bg-paper shadow-sm"
    >
      <div className="flex min-h-16 items-center gap-3 border-b border-line px-4 py-3">
        <div className="flex min-w-0 flex-1 items-center gap-3">
          <span className="grid size-10 shrink-0 place-items-center rounded-xl bg-paper-hover text-ink-soft">
            <FilePenLine size={20} />
          </span>
          <span className="min-w-0">
            <span className="block truncate text-base font-medium text-ink">{title}</span>
            <FileStats additions={additions} deletions={deletions} className="mt-0.5" />
          </span>
        </div>

        {onUndoFileChanges || onReapplyFileChanges ? (
          <button
            type="button"
            data-file-change-undo={!allUndone ? 'true' : undefined}
            data-file-change-reapply={allUndone ? 'true' : undefined}
            disabled={mixedState || operation !== null || (allUndone ? !onReapplyFileChanges : !onUndoFileChanges)}
            onClick={() => void applyFileChangeAction()}
            className="inline-flex min-h-9 items-center gap-1.5 rounded-lg px-2.5 text-sm text-ink transition-colors hover:bg-paper-hover disabled:cursor-not-allowed disabled:text-ink-faint"
          >
            {mixedState ? (
              <Check size={15} />
            ) : allUndone ? (
              <Redo2 size={15} className={operation === 'reapply' ? 'animate-spin' : ''} />
            ) : (
              <RotateCcw size={15} className={operation === 'undo' ? 'animate-spin' : ''} />
            )}
            {mixedState
              ? t('tool.undone')
              : allUndone
                ? operation === 'reapply' ? t('tool.reapplying') : t('tool.reapply')
                : operation === 'undo' ? t('tool.undoing') : t('tool.undo')}
          </button>
        ) : null}
        {onReviewFileChanges ? (
          <button
            type="button"
            data-file-change-review="true"
            onClick={() => onReviewFileChanges(changes)}
            className="inline-flex min-h-9 items-center gap-1.5 rounded-xl border border-line px-3 text-sm text-ink transition-colors hover:bg-paper-hover"
          >
            <Eye size={15} />
            {t('tool.review')}
          </button>
        ) : null}
      </div>

      {operationError ? (
        <div role="alert" className="border-b border-line bg-status-danger-soft px-4 py-2 text-xs text-status-danger-ink">
          {t(operationError.kind === 'reapply' ? 'tool.reapplyFailed' : 'tool.undoFailed', {
            message: operationError.message,
          })}
        </div>
      ) : null}

      <div className="divide-y divide-line px-4">
        {visibleChanges.map((change) => (
          <div
            key={change.changeId}
            data-file-change-row={change.changeId}
            className="flex min-w-0 items-center gap-4 py-3"
          >
            <span className="min-w-0 flex-1 truncate text-sm text-ink-soft">
              {change.path}
            </span>
            <FileStats additions={change.additions} deletions={change.deletions} />
          </div>
        ))}
      </div>

      {changes.length > 3 ? (
        <button
          type="button"
          data-file-change-show-more="true"
          aria-expanded={showAll}
          onClick={() => setShowAll((value) => !value)}
          className="flex min-h-11 w-full items-center gap-2 border-t border-line px-4 text-left text-sm text-ink transition-colors hover:bg-paper-hover"
        >
          <span>{showAll
            ? t('tool.showFewerFiles')
            : t('tool.showMoreFiles', { count: hiddenCount })}</span>
          <ChevronDown
            size={16}
            className={`transition-transform ${showAll ? 'rotate-180' : ''}`}
          />
        </button>
      ) : null}
    </div>
  )
}
