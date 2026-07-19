import { useState } from 'react'
import {
  Check,
  ChevronDown,
  Copy,
  Eye,
  FilePenLine,
  RotateCcw,
  X,
} from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { resolveErrorMessage } from '../../../utils/commandError'

export type FileChangeKind = 'created' | 'modified'
export type FileDiffLineKind = 'context' | 'addition' | 'deletion'

export interface FileDiffLine {
  kind: FileDiffLineKind
  oldLine: number | null
  newLine: number | null
  content: string
  noNewline?: boolean
}

export interface FileDiffHunk {
  oldStart: number
  oldLines: number
  newStart: number
  newLines: number
  lines: FileDiffLine[]
}

export interface FileChangeView {
  changeId: string
  path: string
  kind: FileChangeKind
  additions: number
  deletions: number
  hunks: FileDiffHunk[]
  beforeHash: string | null
  afterHash: string
  undone: boolean
}

interface FileChangeCardProps {
  changes: FileChangeView[]
  onUndoFileChanges?: (changeIds: string[]) => Promise<void>
}

export function FileChangeCard({ changes, onUndoFileChanges }: FileChangeCardProps) {
  const { t } = useTranslation()
  const [expanded, setExpanded] = useState(true)
  const [reviewOpen, setReviewOpen] = useState(false)
  const [undoing, setUndoing] = useState(false)
  const [undoError, setUndoError] = useState<string | null>(null)
  const additions = changes.reduce((total, change) => total + change.additions, 0)
  const deletions = changes.reduce((total, change) => total + change.deletions, 0)
  const undone = changes.some((change) => change.undone)
  const title = changes.length === 1
    ? changes[0].kind === 'created'
      ? t('tool.createdFile')
      : t('tool.editedOneFile')
    : t('tool.editedFileCount', { count: changes.length })

  async function undo() {
    if (!onUndoFileChanges || undone || undoing) return
    setUndoing(true)
    setUndoError(null)
    try {
      await onUndoFileChanges(changes.map((change) => change.changeId))
    } catch (error) {
      setUndoError(resolveErrorMessage(error))
    } finally {
      setUndoing(false)
    }
  }

  return (
    <div
      data-file-change-summary="true"
      className="overflow-hidden rounded-2xl border border-line bg-paper shadow-sm"
    >
      <div className="flex min-h-16 items-center gap-3 border-b border-line px-4 py-3">
        <button
          type="button"
          aria-expanded={expanded}
          onClick={() => setExpanded((value) => !value)}
          className="flex min-w-0 flex-1 items-center gap-3 text-left"
        >
          <span className="grid size-10 shrink-0 place-items-center rounded-xl bg-paper-hover text-ink-soft">
            <FilePenLine size={20} />
          </span>
          <span className="min-w-0">
            <span className="block truncate text-base font-medium text-ink">{title}</span>
            <FileStats additions={additions} deletions={deletions} className="mt-0.5" />
          </span>
          <ChevronDown
            size={17}
            className={`ml-1 shrink-0 text-ink-faint transition-transform ${expanded ? '' : '-rotate-90'}`}
          />
        </button>

        {onUndoFileChanges ? (
          <button
            type="button"
            data-file-change-undo="true"
            disabled={undone || undoing}
            onClick={() => void undo()}
            className="inline-flex min-h-9 items-center gap-1.5 rounded-lg px-2.5 text-sm text-ink transition-colors hover:bg-paper-hover disabled:cursor-not-allowed disabled:text-ink-faint"
          >
            {undone ? <Check size={15} /> : <RotateCcw size={15} className={undoing ? 'animate-spin' : ''} />}
            {undone ? t('tool.undone') : undoing ? t('tool.undoing') : t('tool.undo')}
          </button>
        ) : null}
        <button
          type="button"
          data-file-change-review="true"
          onClick={() => setReviewOpen(true)}
          className="inline-flex min-h-9 items-center gap-1.5 rounded-xl border border-line px-3 text-sm text-ink transition-colors hover:bg-paper-hover"
        >
          <Eye size={15} />
          {t('tool.review')}
        </button>
      </div>

      {undoError ? (
        <div role="alert" className="border-b border-line bg-status-danger-soft px-4 py-2 text-xs text-status-danger-ink">
          {t('tool.undoFailed', { message: undoError })}
        </div>
      ) : null}

      {expanded ? (
        changes.length === 1 ? (
          <DiffPanel change={changes[0]} />
        ) : (
          <div className="divide-y divide-line px-4">
            {changes.map((change) => (
              <div key={change.changeId} className="flex min-w-0 items-center gap-4 py-3">
                <span className="min-w-0 flex-1 truncate font-mono text-sm text-ink-soft">
                  {change.path}
                </span>
                <FileStats additions={change.additions} deletions={change.deletions} />
              </div>
            ))}
          </div>
        )
      ) : null}

      {reviewOpen ? (
        <ReviewDialog changes={changes} onClose={() => setReviewOpen(false)} />
      ) : null}
    </div>
  )
}

function ReviewDialog({ changes, onClose }: { changes: FileChangeView[]; onClose: () => void }) {
  const { t } = useTranslation()
  return (
    <div
      className="fixed inset-0 z-50 grid place-items-center bg-black/45 p-4"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose()
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={t('tool.reviewChanges')}
        className="flex max-h-[88vh] w-full max-w-6xl flex-col overflow-hidden rounded-2xl border border-line bg-paper shadow-2xl"
        onKeyDown={(event) => {
          if (event.key === 'Escape') onClose()
        }}
      >
        <div className="flex items-center border-b border-line px-5 py-4">
          <div className="min-w-0 flex-1">
            <h2 className="text-lg font-semibold text-ink">{t('tool.reviewChanges')}</h2>
            <p className="mt-0.5 text-xs text-ink-faint">
              {t('tool.fileChangeCount', { count: changes.length })}
            </p>
          </div>
          <button
            type="button"
            aria-label={t('tool.closeReview')}
            onClick={onClose}
            className="grid size-9 place-items-center rounded-lg text-ink-soft hover:bg-paper-hover"
          >
            <X size={18} />
          </button>
        </div>
        <div className="space-y-4 overflow-auto p-4">
          {changes.map((change) => (
            <div key={change.changeId} className="overflow-hidden rounded-xl border border-line">
              <DiffPanel change={change} />
            </div>
          ))}
        </div>
      </div>
    </div>
  )
}

function DiffPanel({ change }: { change: FileChangeView }) {
  const { t } = useTranslation()

  async function copyDiff() {
    await navigator.clipboard?.writeText(formatPatch(change))
  }

  return (
    <div data-file-change={change.changeId} className="min-w-0 bg-paper">
      <div className="flex min-h-11 items-center gap-3 border-b border-line bg-paper-hover/70 px-3">
        <span className="min-w-0 flex-1 truncate font-mono text-sm text-ink-soft">{change.path}</span>
        <FileStats additions={change.additions} deletions={change.deletions} />
        <button
          type="button"
          aria-label={t('tool.copyDiff')}
          title={t('tool.copyDiff')}
          onClick={() => void copyDiff()}
          className="grid size-8 shrink-0 place-items-center rounded-md text-ink-faint hover:bg-paper hover:text-ink"
        >
          <Copy size={15} />
        </button>
      </div>
      <div className="max-h-[58vh] overflow-auto bg-code-bg font-mono text-[12px] leading-5">
        {change.hunks.map((hunk, hunkIndex) => (
          <div key={`${change.changeId}-${hunkIndex}`}>
            <div className="border-y border-line bg-clay-soft/40 px-3 py-1 text-clay">
              @@ -{hunk.oldStart},{hunk.oldLines} +{hunk.newStart},{hunk.newLines} @@
            </div>
            {hunk.lines.map((line, lineIndex) => (
              <DiffLineRow key={lineIndex} line={line} />
            ))}
          </div>
        ))}
      </div>
    </div>
  )
}

function DiffLineRow({ line }: { line: FileDiffLine }) {
  const marker = line.kind === 'addition' ? '+' : line.kind === 'deletion' ? '-' : ' '
  const classes = line.kind === 'addition'
    ? 'bg-status-success-soft text-status-success-ink'
    : line.kind === 'deletion'
      ? 'bg-status-danger-soft text-status-danger-ink'
      : 'text-ink-soft'
  return (
    <>
      <div className={`grid min-w-max grid-cols-[3.25rem_3.25rem_1.25rem_minmax(0,1fr)] ${classes}`}>
        <span className="select-none border-r border-line/70 px-2 text-right text-ink-faint">
          {line.oldLine ?? ''}
        </span>
        <span className="select-none border-r border-line/70 px-2 text-right text-ink-faint">
          {line.newLine ?? ''}
        </span>
        <span className="select-none text-center">{marker}</span>
        <span className="whitespace-pre pr-4">{line.content}</span>
      </div>
      {line.noNewline ? (
        <div className="px-[8rem] text-[10px] italic text-ink-faint">\ No newline at end of file</div>
      ) : null}
    </>
  )
}

function FileStats({
  additions,
  deletions,
  className = '',
}: {
  additions: number
  deletions: number
  className?: string
}) {
  return (
    <span className={`inline-flex shrink-0 items-center gap-1.5 font-mono text-sm ${className}`}>
      <span className="text-status-success">+{additions}</span>
      <span className="text-status-danger">-{deletions}</span>
    </span>
  )
}

function formatPatch(change: FileChangeView): string {
  const header = change.kind === 'created'
    ? `--- /dev/null\n+++ b/${change.path}`
    : `--- a/${change.path}\n+++ b/${change.path}`
  const hunks = change.hunks.map((hunk) => {
    const lines = hunk.lines.map((line) => {
      const marker = line.kind === 'addition' ? '+' : line.kind === 'deletion' ? '-' : ' '
      return `${marker}${line.content}${line.noNewline ? '\n\\ No newline at end of file' : ''}`
    })
    return `@@ -${hunk.oldStart},${hunk.oldLines} +${hunk.newStart},${hunk.newLines} @@\n${lines.join('\n')}`
  })
  return [header, ...hunks].join('\n')
}
