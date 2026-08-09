import { useId, useState } from 'react'
import {
  Check,
  ChevronDown,
  CircleAlert,
  CircleX,
  Copy,
  FilePlusCorner,
  Loader2,
  Pencil,
  Redo2,
  RotateCcw,
} from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { resolveErrorMessage } from '@/lib/commandError'
import {
  isFailure,
  isInProgress,
  type ToolActivity,
} from '../toolActivity'
import {
  Separator,
  ToolActivityFrame,
} from './ToolActivityFrame'
import { FileChangePathLabel, projectFilePath } from './FileChangePathLabel'

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

interface FileChangeActivityRowProps {
  activities: ToolActivity[]
  activityChanges: FileChangeView[][]
  turnActive: boolean
  expanded?: boolean
  onExpandedChange: (expanded: boolean) => void
  onOpenTrace?: (providerToolCallId: string) => void
  onUndoFileChanges?: (changeIds: string[]) => Promise<void>
  onReapplyFileChanges?: (changeIds: string[]) => Promise<void>
  workspaceRoot?: string
}

export function isFileChangeDisplayTool(name: string): name is 'edit' | 'write' {
  return name === 'edit' || name === 'write'
}

/**
 * 对话流中的文件变更卡片与独立 Diff 共用同一份代码渲染。
 * 这里额外处理工具状态、相邻调用聚合和撤销生命周期，但不会再包一层重复的文件头部。
 */
export function FileChangeActivityRow({
  activities,
  activityChanges,
  turnActive,
  expanded: rememberedExpanded,
  onExpandedChange,
  onOpenTrace,
  onUndoFileChanges,
  onReapplyFileChanges,
  workspaceRoot,
}: FileChangeActivityRowProps) {
  const { t } = useTranslation()
  const [operation, setOperation] = useState<'undo' | 'reapply' | null>(null)
  const [operationError, setOperationError] = useState<string | null>(null)
  const primary = activities[0]
  const changes = activityChanges.flat()
  const failed = activities.some((activity) => isFailure(activity))
  const statusActivity = activities.find((activity) => isInProgress(activity)) ?? primary
  const expanded = failed || (rememberedExpanded ?? true)
  const paths = [...new Set([
    ...activities.map(activityPath).filter(Boolean),
    ...changes.map((change) => change.path),
  ])]
  const target = paths[0] ? projectFilePath(paths[0], workspaceRoot) : ''
  const multipleFiles = paths.length > 1
  const additions = changes.reduce((total, change) => total + change.additions, 0)
  const deletions = changes.reduce((total, change) => total + change.deletions, 0)
  const hunkCount = changes.reduce((total, change) => total + change.hunks.length, 0)
  const createdWrite = primary.name === 'write'
    && changes.length > 0
    && changes.every((change) => change.kind === 'created')
  const allUndone = changes.length > 0 && changes.every((change) => change.undone)
  const mixedState = changes.some((change) => change.undone) && !allUndone
  const action = changes.length > 0 && changes.every((change) => change.kind === 'created')
    ? t('tool.mutation.createAction')
    : primary.name === 'edit'
      ? t('tool.mutation.editAction')
      : t('tool.mutation.writeAction')

  async function applyFileChangeAction() {
    if (turnActive || operation || mixedState || changes.length === 0) return
    const kind = allUndone ? 'reapply' : 'undo'
    const callback = allUndone ? onReapplyFileChanges : onUndoFileChanges
    if (!callback) return
    setOperation(kind)
    setOperationError(null)
    try {
      await callback(changes.map((change) => change.changeId))
    } catch (error) {
      setOperationError(resolveErrorMessage(error))
    } finally {
      setOperation(null)
    }
  }

  async function copyFailure() {
    const error = activities.map((activity) => activity.output).filter(Boolean).join('\n\n')
    try {
      await navigator.clipboard?.writeText(error)
    } catch {
      // 剪贴板不可用时保留可选择的错误文本。
    }
  }

  const summary = (
    <>
      <span className="shrink-0 text-xs font-medium text-ink-soft" title={primary.name}>
        <span data-file-change-activity-summary="true">{multipleFiles
          ? `${action} ${t('tool.mutation.fileCount', { count: paths.length })}`
          : action}</span>
      </span>
      {!multipleFiles && target ? <Separator /> : null}
      {!multipleFiles && target ? (
        <span className="min-w-0 truncate font-mono text-xs text-ink" title={target}>
          {target}
        </span>
      ) : null}
      {!multipleFiles && activities.length > 1 ? <Separator /> : null}
      {!multipleFiles && activities.length > 1 ? <span className="shrink-0 text-xs text-ink-faint">×{activities.length}</span> : null}
      {primary.name === 'edit' && hunkCount > 0 ? <Separator /> : null}
      {primary.name === 'edit' && hunkCount > 0 ? (
        <span className="shrink-0 text-xs text-ink-faint">
          {t('tool.mutation.changeCount', { count: hunkCount })}
        </span>
      ) : null}
      {changes.length > 0 ? <Separator /> : null}
      {createdWrite ? (
        <span className="shrink-0 font-mono text-xs text-status-success">
          {t('tool.mutation.addedLines', { count: additions })}
        </span>
      ) : changes.length > 0 ? (
        <FileStats additions={additions} deletions={deletions} className="text-xs" />
      ) : null}
    </>
  )

  const undoAction = changes.length > 0 && (onUndoFileChanges || onReapplyFileChanges) ? (
    <button
      type="button"
      data-tool-undo={!allUndone ? changes.map((change) => change.changeId).join(' ') : undefined}
      data-tool-reapply={allUndone ? changes.map((change) => change.changeId).join(' ') : undefined}
      disabled={turnActive || mixedState || operation !== null || (allUndone ? !onReapplyFileChanges : !onUndoFileChanges)}
      title={turnActive ? t('tool.mutation.availableAfterTurn') : undefined}
      onClick={() => void applyFileChangeAction()}
      className="mr-1 inline-flex min-h-7 shrink-0 items-center gap-1 rounded-full border border-status-success-border bg-paper/45 px-2.5 text-xs text-status-success-ink hover:bg-paper disabled:border-line disabled:text-ink-faint"
    >
      {mixedState ? (
        <Check size={12} />
      ) : allUndone ? (
        <Redo2 size={12} className={operation === 'reapply' ? 'animate-spin' : ''} />
      ) : (
        <RotateCcw size={12} className={operation === 'undo' ? 'animate-spin' : ''} />
      )}
      {mixedState
        ? t('tool.undone')
        : allUndone
          ? operation === 'reapply' ? t('tool.reapplying') : t('tool.reapply')
          : operation === 'undo' ? t('tool.undoing') : t('tool.undo')}
    </button>
  ) : null

  return (
    <ToolActivityFrame
      toolCallId={primary.id}
      tier={failed ? 'failure' : 'write'}
      expanded={expanded}
      onExpandedChange={failed ? undefined : onExpandedChange}
      statusIcon={<FileChangeStatusIcon activity={statusActivity} />}
      summary={summary}
      inlineActions={undoAction}
      showDisclosure={false}
      notice={operationError ? (
        <div role="alert" className="border-t border-status-danger-border bg-status-danger-soft px-3 py-2 text-xs text-status-danger-ink">
          {operationError}
        </div>
      ) : null}
      onOpenTrace={onOpenTrace}
      detailsMaxHeightClass="max-h-[240px]"
      dataFileChangeActivity={changes[0]?.changeId}
    >
      <div className="bg-paper/70">
        {primary.name === 'write' ? (
          activities.map((activity, index) => {
            const writeChanges = activityChanges[index] ?? []
            if (shouldRenderWriteDiff(activity, writeChanges)) {
              return (
                <div key={activity.id} data-write-diff={activity.id}>
                  {writeChanges.map((change) => (
                    <ActivityFileDiff
                      key={change.changeId}
                      change={change}
                      grouped={activities.length > 1}
                      workspaceRoot={workspaceRoot}
                    />
                  ))}
                </div>
              )
            }
            return (
              <WritePreview
                key={activity.id}
                activity={activity}
                grouped={activities.length > 1}
                workspaceRoot={workspaceRoot}
              />
            )
          })
        ) : changes.map((change) => (
          <ActivityFileDiff
            key={change.changeId}
            change={change}
            grouped={activities.length > 1}
            workspaceRoot={workspaceRoot}
          />
        ))}
        {changes.length === 0 && primary.output ? (
          <pre className={`whitespace-pre-wrap break-words px-3 py-2 font-mono text-xs ${failed ? 'text-status-danger-ink' : 'text-ink-soft'}`}>
            {primary.output}
          </pre>
        ) : null}
        {failed ? (
          <div className="flex items-center gap-2 border-t border-status-danger-border bg-paper px-3 py-2">
            <button
              type="button"
              onClick={() => void copyFailure()}
              className="inline-flex items-center gap-1.5 rounded-md border border-status-danger-border px-2.5 py-1 text-xs text-status-danger-ink hover:bg-status-danger-soft"
            >
              <Copy size={11} />
              {t('tool.bash.copyError')}
            </button>
            {onOpenTrace ? (
              <button
                type="button"
                onClick={() => onOpenTrace(primary.id)}
                className="rounded-md border border-status-danger-border px-2.5 py-1 text-xs text-status-danger-ink hover:bg-status-danger-soft"
              >
                {t('tool.bash.inspectTrace')}
              </button>
            ) : null}
          </div>
        ) : null}
      </div>
    </ToolActivityFrame>
  )
}

/**
 * 单个文件的差异面板。
 *
 * 摘要卡、工具行的展开区、审阅抽屉三处都用它，所以它独立成模块 —— 否则审阅抽屉
 * 要从 FileChangeCard 拿这个组件，而 FileChangeCard 又要拿抽屉，形成循环 import。
 */
export function FileDiffPanel({
  change,
  hunkLabel = 'patch',
  compact = false,
  defaultExpanded,
  showCopyAction,
  workspaceRoot,
}: {
  change: FileChangeView
  hunkLabel?: 'patch' | 'start'
  compact?: boolean
  defaultExpanded?: boolean
  showCopyAction?: boolean
  workspaceRoot?: string
}) {
  const { t } = useTranslation()
  const [expanded, setExpanded] = useState(defaultExpanded ?? !compact)
  const contentId = useId()
  const copyEnabled = showCopyAction ?? !compact

  async function copyDiff() {
    await navigator.clipboard?.writeText(formatPatch(change))
  }

  return (
    <div
      data-file-change={change.changeId}
      data-file-change-compact={compact ? 'true' : undefined}
      className={`min-w-0 bg-paper ${compact ? 'border-b border-line last:border-b-0' : ''}`}
    >
      <div
        className={`flex items-center gap-2 bg-paper-hover/70 px-3 ${compact ? 'min-h-9' : 'min-h-11'} ${expanded ? 'border-b border-line' : ''}`}
      >
        <button
          type="button"
          data-file-change-toggle="true"
          aria-expanded={expanded}
          aria-controls={contentId}
          aria-label={t(expanded ? 'tool.collapseDiff' : 'tool.expandDiff', { name: change.path })}
          title={t(expanded ? 'tool.collapseDiff' : 'tool.expandDiff', { name: change.path })}
          onClick={() => setExpanded((value) => !value)}
          className={`flex min-w-0 flex-1 items-center text-left ${compact ? 'min-h-9 gap-2' : 'min-h-11 gap-3'}`}
        >
          {compact ? (
            <FileChangePathLabel
              path={change.path}
              workspaceRoot={workspaceRoot}
              additions={change.additions}
              deletions={change.deletions}
            />
          ) : (
            <span className="min-w-0 flex-1 truncate font-mono text-sm text-ink-soft">{change.path}</span>
          )}
          <FileStats
            additions={change.additions}
            deletions={change.deletions}
            className={compact ? 'text-xs' : ''}
          />
          <ChevronDown
            size={compact ? 13 : 16}
            className={`shrink-0 text-ink-faint transition-transform duration-200 ${expanded ? 'rotate-180' : ''}`}
          />
        </button>
        {copyEnabled ? (
          <button
            type="button"
            aria-label={t('tool.copyDiff')}
            title={t('tool.copyDiff')}
            onClick={() => void copyDiff()}
            className={`grid shrink-0 place-items-center rounded-md text-ink-faint hover:bg-paper hover:text-ink ${compact ? 'size-7' : 'size-8'}`}
          >
            <Copy size={15} />
          </button>
        ) : null}
      </div>
      <FileDiffContent
        change={change}
        hunkLabel={hunkLabel}
        contentId={contentId}
        hidden={!expanded}
        scrollable
      />
    </div>
  )
}

function ActivityFileDiff({
  change,
  grouped,
  workspaceRoot,
}: {
  change: FileChangeView
  grouped: boolean
  workspaceRoot?: string
}) {
  if (grouped) return (
    <FileDiffPanel
      change={change}
      hunkLabel="start"
      compact
      workspaceRoot={workspaceRoot}
    />
  )
  return (
    <div data-file-change={change.changeId} className="min-w-0 bg-paper">
      <FileDiffContent change={change} hunkLabel="start" />
    </div>
  )
}

/** 执行后的文件变更与执行前的审批预览共用行渲染，避免两套 Diff 视觉逐渐分叉。 */
export function FileDiffContent({
  change,
  hunkLabel = 'patch',
  contentId,
  hidden = false,
  scrollable = false,
  showHunkHeaders = true,
}: {
  change: Pick<FileChangeView, 'changeId' | 'hunks'>
  hunkLabel?: 'patch' | 'start'
  contentId?: string
  hidden?: boolean
  scrollable?: boolean
  showHunkHeaders?: boolean
}) {
  const { t } = useTranslation()
  return (
    <div
      id={contentId}
      data-file-change-code="true"
      hidden={hidden}
      className={`${scrollable ? 'max-h-[58vh] overflow-auto ' : ''}bg-code-bg font-mono text-[12px] leading-5`}
    >
      {change.hunks.map((hunk, hunkIndex) => (
        <div key={`${change.changeId}-${hunkIndex}`}>
          {showHunkHeaders ? (
            <div className="border-y border-line bg-clay-soft/40 px-3 py-1 text-clay">
              {hunkLabel === 'start'
                ? `@@ ${t('tool.mutation.hunkStart', { line: hunk.newStart })}`
                : `@@ -${hunk.oldStart},${hunk.oldLines} +${hunk.newStart},${hunk.newLines} @@`}
            </div>
          ) : null}
          {hunk.lines.map((line, lineIndex) => (
            <DiffLineRow key={lineIndex} line={line} />
          ))}
        </div>
      ))}
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

function WritePreview({
  activity,
  grouped,
  workspaceRoot,
}: {
  activity: ToolActivity
  grouped: boolean
  workspaceRoot?: string
}) {
  const { t } = useTranslation()
  const [showAll, setShowAll] = useState(false)
  const content = typeof activity.input?.content === 'string' ? activity.input.content : null
  if (content == null) return null
  const lines = content.split('\n')
  const visible = showAll ? lines : lines.slice(0, 6)
  const hidden = lines.length - visible.length

  return (
    <div data-write-preview={activity.id} className="bg-code-bg font-mono text-xs leading-5">
      {grouped ? <WritePreviewHeading activity={activity} workspaceRoot={workspaceRoot} /> : null}
      {content === '' ? (
        <div data-write-empty="true" className="px-3 py-2 text-ink-faint">
          {t('tool.mutation.emptyFile')}
        </div>
      ) : (
        <pre className="overflow-x-auto whitespace-pre px-3 py-2 text-ink-soft">
          {visible.join('\n')}
        </pre>
      )}
      {lines.length > 6 ? (
        <button
          type="button"
          aria-expanded={showAll}
          onClick={() => setShowAll((value) => !value)}
          className="w-full border-t border-line px-3 py-1.5 text-left text-xs text-clay hover:bg-paper-hover"
        >
          {showAll
            ? t('tool.mutation.collapseFullContent')
            : t('tool.mutation.expandFullContent', { count: hidden })}
        </button>
      ) : null}
    </div>
  )
}

function shouldRenderWriteDiff(activity: ToolActivity, changes: FileChangeView[]): boolean {
  if (changes.length === 0) return false
  const content = typeof activity.input?.content === 'string' ? activity.input.content : null
  return content !== '' || changes.some((change) => change.kind === 'modified' || change.additions > 0)
}

function WritePreviewHeading({
  activity,
  workspaceRoot,
}: {
  activity: ToolActivity
  workspaceRoot?: string
}) {
  const path = activityPath(activity)
  return (
    <div className="truncate border-b border-line bg-paper px-3 py-1.5 text-ink-soft" title={path}>
      {projectFilePath(path, workspaceRoot)}
    </div>
  )
}

function FileChangeStatusIcon({ activity }: { activity: ToolActivity }) {
  const { t } = useTranslation()
  if (activity.state === 'pending' || activity.state === 'submitted' || activity.state === 'running') {
    return <Loader2 size={14} aria-label={t('tool.running')} className="shrink-0 animate-spin text-ink-faint" />
  }
  if (activity.state === 'error') {
    return <CircleAlert size={14} aria-label={t('tool.error')} className="shrink-0 text-status-danger" />
  }
  if (activity.state === 'denied' || activity.state === 'interrupted') {
    return <CircleX size={14} aria-label={t('tool.stopped')} className="shrink-0 text-status-danger" />
  }
  const Icon = activity.name === 'write' ? FilePlusCorner : Pencil
  // 成功态图标是装饰：动作名就在紧邻的摘要里，读屏再念一遍只是噪音。
  return (
    <span aria-hidden="true" className="grid size-5 shrink-0 place-items-center rounded-full bg-status-success text-paper">
      <Icon size={11} strokeWidth={2.2} />
    </span>
  )
}

function activityPath(activity: ToolActivity): string {
  const path = activity.input?.path ?? activity.input?.filePath
  return typeof path === 'string' ? path : ''
}
