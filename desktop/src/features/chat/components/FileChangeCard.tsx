import { useState } from 'react'
import { Check, ChevronDown, Eye, FilePenLine, Redo2, RotateCcw } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { resolveErrorMessage } from '@/lib/commandError'
import { FileStats, type FileChangeView } from './FileDiffPanel'
import { FileChangePathLabel, workspaceDisplayName } from './FileChangePathLabel'

const COLLAPSED_FILE_LIMIT = 3

interface FileChangeSummaryRow {
  changeIds: string[]
  path: string
  kind: FileChangeView['kind']
  additions: number
  deletions: number
}

function summarizeFiles(changes: FileChangeView[]): FileChangeSummaryRow[] {
  const byPath = new Map<string, FileChangeSummaryRow>()
  for (const change of changes) {
    const previous = byPath.get(change.path)
    byPath.set(change.path, previous ? {
      changeIds: [...previous.changeIds, change.changeId],
      path: change.path,
      kind: previous.kind === 'created' && change.kind === 'created' ? 'created' : 'modified',
      additions: previous.additions + change.additions,
      deletions: previous.deletions + change.deletions,
    } : {
      changeIds: [change.changeId],
      path: change.path,
      kind: change.kind,
      additions: change.additions,
      deletions: change.deletions,
    })
  }
  return [...byPath.values()]
}

interface FileChangeCardProps {
  changes: FileChangeView[]
  onUndoFileChanges?: (changeIds: string[]) => Promise<void>
  onReapplyFileChanges?: (changeIds: string[]) => Promise<void>
  /// 审阅面板是页面右栏，不归卡片管，卡片只负责把这批改动递上去。
  onReviewFileChanges?: (changes: FileChangeView[]) => void
  workspaceRoot?: string
}

export function FileChangeCard({
  changes,
  onUndoFileChanges,
  onReapplyFileChanges,
  onReviewFileChanges,
  workspaceRoot,
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
  const files = summarizeFiles(changes)
  const someUndone = changes.some((change) => change.undone)
  const allUndone = changes.length > 0 && changes.every((change) => change.undone)
  const mixedState = someUndone && !allUndone
  const visibleChanges = showAll ? files : files.slice(0, COLLAPSED_FILE_LIMIT)
  const hiddenCount = files.length - visibleChanges.length
  const projectName = workspaceDisplayName(workspaceRoot)
  const title = files.length === 1
    ? files[0].kind === 'created'
      ? t('tool.createdFile')
      : t('tool.editedOneFile')
    : t('tool.editedFileCount', { count: files.length })

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
      className="overflow-hidden rounded-xl border border-line bg-paper shadow-sm"
    >
      <div className="flex min-h-14 items-center gap-2.5 border-b border-line px-3 py-2.5">
        <div className="flex min-w-0 flex-1 items-center gap-2.5">
          <span className="grid size-8 shrink-0 place-items-center rounded-full bg-status-success-soft text-status-success-ink">
            <FilePenLine size={16} />
          </span>
          <span className="min-w-0">
            <span className="block truncate text-sm font-semibold text-ink">{title}</span>
            <span className="mt-0.5 flex min-w-0 items-center gap-2 text-xs">
              <FileStats additions={additions} deletions={deletions} className="text-xs" />
              {projectName ? (
                <span className="truncate text-ink-faint">
                  {t('tool.inProject', { name: projectName })}
                </span>
              ) : null}
            </span>
          </span>
        </div>

        {onUndoFileChanges || onReapplyFileChanges ? (
          <button
            type="button"
            data-file-change-undo={!allUndone ? 'true' : undefined}
            data-file-change-reapply={allUndone ? 'true' : undefined}
            disabled={mixedState || operation !== null || (allUndone ? !onReapplyFileChanges : !onUndoFileChanges)}
            onClick={() => void applyFileChangeAction()}
            className="inline-flex min-h-8 shrink-0 items-center gap-1.5 rounded-full border border-line px-3 text-xs text-ink transition-colors hover:bg-paper-hover disabled:cursor-not-allowed disabled:text-ink-faint"
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
            className="inline-flex min-h-8 shrink-0 items-center gap-1.5 rounded-full bg-clay px-3 text-xs font-medium text-paper transition-colors hover:opacity-90"
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

      <div className="divide-y divide-line">
        {visibleChanges.map((change) => (
          <div
            key={change.path}
            data-file-change-row={change.changeIds.join(' ')}
            className="flex min-h-9 min-w-0 items-center gap-3 px-3 py-1.5"
          >
            <FileChangePathLabel
              path={change.path}
              workspaceRoot={workspaceRoot}
              additions={change.additions}
              deletions={change.deletions}
            />
            <FileStats additions={change.additions} deletions={change.deletions} className="text-xs" />
          </div>
        ))}
      </div>

      {files.length > COLLAPSED_FILE_LIMIT ? (
        <button
          type="button"
          data-file-change-show-more="true"
          aria-expanded={showAll}
          onClick={() => setShowAll((value) => !value)}
          className="flex min-h-9 w-full items-center gap-2 border-t border-line px-3 text-left text-xs text-ink transition-colors hover:bg-paper-hover"
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
