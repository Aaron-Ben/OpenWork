import { useId, useState } from 'react'
import { ChevronDown, Copy } from 'lucide-react'
import { useTranslation } from 'react-i18next'

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

/**
 * 单个文件的差异面板。
 *
 * 摘要卡、工具行的展开区、审阅抽屉三处都用它，所以它独立成模块 —— 否则审阅抽屉
 * 要从 FileChangeCard 拿这个组件，而 FileChangeCard 又要拿抽屉，形成循环 import。
 */
export function FileDiffPanel({ change }: { change: FileChangeView }) {
  const { t } = useTranslation()
  const [expanded, setExpanded] = useState(true)
  const contentId = useId()

  async function copyDiff() {
    await navigator.clipboard?.writeText(formatPatch(change))
  }

  return (
    <div data-file-change={change.changeId} className="min-w-0 bg-paper">
      <div
        className={`flex min-h-11 items-center gap-2 bg-paper-hover/70 px-3 ${expanded ? 'border-b border-line' : ''}`}
      >
        <button
          type="button"
          data-file-change-toggle="true"
          aria-expanded={expanded}
          aria-controls={contentId}
          aria-label={t(expanded ? 'tool.collapseDiff' : 'tool.expandDiff', { name: change.path })}
          title={t(expanded ? 'tool.collapseDiff' : 'tool.expandDiff', { name: change.path })}
          onClick={() => setExpanded((value) => !value)}
          className="flex min-h-11 min-w-0 flex-1 items-center gap-3 text-left"
        >
          <span className="min-w-0 flex-1 truncate font-mono text-sm text-ink-soft">{change.path}</span>
          <FileStats additions={change.additions} deletions={change.deletions} />
          <ChevronDown
            size={16}
            className={`shrink-0 text-ink-faint transition-transform duration-200 ${expanded ? 'rotate-180' : ''}`}
          />
        </button>
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
      <div
        id={contentId}
        data-file-change-code="true"
        hidden={!expanded}
        className="max-h-[58vh] overflow-auto bg-code-bg font-mono text-[12px] leading-5"
      >
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
  const lineNumber = line.kind === 'deletion'
    ? line.oldLine
    : line.newLine ?? line.oldLine
  const classes = line.kind === 'addition'
    ? 'bg-status-success-soft text-status-success-ink'
    : line.kind === 'deletion'
      ? 'bg-status-danger-soft text-status-danger-ink'
      : 'text-ink-soft'
  const lineNumberClass = line.kind === 'addition'
    ? 'text-status-success'
    : line.kind === 'deletion'
      ? 'text-status-danger'
      : 'text-ink-faint'
  return (
    <>
      <div className={`grid min-w-max grid-cols-[3.25rem_1.25rem_minmax(0,1fr)] ${classes}`}>
        <span
          data-diff-line-number={lineNumber ?? undefined}
          className={`select-none border-r border-line/70 px-2 text-right ${lineNumberClass}`}
        >
          {lineNumber ?? ''}
        </span>
        <span className="select-none text-center">{marker}</span>
        <span className="whitespace-pre pr-4">{line.content}</span>
      </div>
      {line.noNewline ? (
        <div className="px-[4.75rem] text-[10px] italic text-ink-faint">\ No newline at end of file</div>
      ) : null}
    </>
  )
}

export function FileStats({
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
