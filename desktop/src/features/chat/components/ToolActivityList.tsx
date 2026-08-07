import { memo, useEffect, useMemo, useRef, useState } from 'react'
import { AnimatePresence, motion } from 'motion/react'
import { useTranslation } from 'react-i18next'
import {
  Activity,
  Check,
  ChevronRight,
  CircleAlert,
  CircleX,
  Copy,
  FileText,
  Folder,
  Loader2,
  Pencil,
  SquareTerminal,
  Wrench,
} from 'lucide-react'

import {
  extractText,
  type ContentBlock,
  type ToolResultArtifact,
  type ToolResultState,
} from '@/types/parts'
import {
  FileChangeCard,
  FileDiffPanel,
  FileStats,
  type FileChangeView,
  type FileDiffHunk,
  type FileDiffLine,
} from './FileChangeCard'

type ActivityState = ToolResultState | 'pending' | 'submitted' | 'finished'

export interface ToolActivity {
  id: string
  name: string
  input: Record<string, unknown> | null
  rawInput: string
  summary: string
  output: string
  state: ActivityState
  artifacts: ToolResultArtifact[]
}

interface ToolActivityListProps {
  parts: ContentBlock[]
  onOpenTrace?: (providerToolCallId: string) => void
  onUndoFileChanges?: (changeIds: string[]) => Promise<void>
  onReapplyFileChanges?: (changeIds: string[]) => Promise<void>
  onReviewFileChanges?: (changes: FileChangeView[]) => void
  fileChangePresentation?: 'activity' | 'summary'
}

const TOOL_ICONS: Record<string, typeof SquareTerminal> = {
  read: FileText,
  write: Pencil,
  edit: Pencil,
  list: Folder,
  bash: SquareTerminal,
}

export function collectToolActivities(parts: ContentBlock[]): ToolActivity[] {
  const order: string[] = []
  const activities = new Map<string, ToolActivity>()

  for (const part of parts) {
    if (part.type === 'tool_call') {
      const input = parseInput(part.input)
      const existing = activities.get(part.id)
      if (!existing) order.push(part.id)
      activities.set(part.id, {
        id: part.id,
        name: part.name,
        input,
        rawInput: part.input,
        summary: summarize(part.name, input),
        output: existing?.output ?? '',
        state: existing?.state ?? part.state,
        artifacts: existing?.artifacts ?? [],
      })
      continue
    }

    if (part.type === 'tool_result') {
      const existing = activities.get(part.id)
      if (!existing) order.push(part.id)
      activities.set(part.id, {
        id: part.id,
        name: existing?.name ?? part.name,
        input: existing?.input ?? null,
        rawInput: existing?.rawInput ?? '',
        summary: existing?.summary ?? '',
        output: extractText(part.output),
        state: part.state,
        artifacts: part.artifacts ?? [],
      })
    }
  }

  return order.flatMap((id) => {
    const activity = activities.get(id)
    return activity ? [activity] : []
  })
}

export function collectFileChanges(activities: ToolActivity[]): FileChangeView[] {
  return activities.flatMap((activity) =>
    activity.artifacts.flatMap((artifact) => {
      const change = parseFileChange(artifact)
      return change ? [change] : []
    }),
  )
}

export const ToolActivityList = memo(function ToolActivityList({
  parts,
  onOpenTrace,
  onUndoFileChanges,
  onReapplyFileChanges,
  onReviewFileChanges,
  fileChangePresentation = 'activity',
}: ToolActivityListProps) {
  const allActivities = useMemo(() => collectToolActivities(parts), [parts])
  const fileChanges = useMemo(() => collectFileChanges(allActivities), [allActivities])
  const fileActivityIds = useMemo(
    () => new Set(
      allActivities
        .filter((activity) => collectFileChanges([activity]).length > 0)
        .map((activity) => activity.id),
    ),
    [allActivities],
  )
  const activities = useMemo(
    () => fileChangePresentation === 'summary'
      ? allActivities.filter((activity) => !fileActivityIds.has(activity.id))
      : allActivities,
    [allActivities, fileActivityIds, fileChangePresentation],
  )

  if (allActivities.length === 0) return null

  return (
    // 同上：不带 py-*，纵向留白由 transcriptSpacing 统一给。
    <div data-tool-activity-list="true" className="w-full space-y-1 text-sm text-ink-soft">
      {fileChangePresentation === 'summary' && fileChanges.length > 0 ? (
        <FileChangeCard
          changes={fileChanges}
          onUndoFileChanges={onUndoFileChanges}
          onReapplyFileChanges={onReapplyFileChanges}
          onReviewFileChanges={onReviewFileChanges}
        />
      ) : null}
      {activities.length > 0 ? (
        <div className="space-y-0.5">
          {activities.map((activity) => (
            <ToolActivityRow
              key={activity.id}
              activity={activity}
              onOpenTrace={onOpenTrace}
            />
          ))}
        </div>
      ) : null}
    </div>
  )
})

function ToolActivityRow({
  activity,
  onOpenTrace,
}: {
  activity: ToolActivity
  onOpenTrace?: (providerToolCallId: string) => void
}) {
  const { t } = useTranslation()
  const [expanded, setExpanded] = useState(false)
  const fileChanges = collectFileChanges([activity])
  const primaryFileChange = fileChanges[0]
  const details = activityDetails(activity, (key) => t(key))
  const hasDetails = fileChanges.length > 0 || details.length > 0
  const label = primaryFileChange
    ? expanded
      ? primaryFileChange.kind === 'created'
        ? t('tool.createdFile')
        : t('tool.editedOneFile')
      : primaryFileChange.kind === 'created'
        ? t('tool.createdNamedFile', { name: fileName(primaryFileChange.path) })
        : t('tool.editedFile', { name: fileName(primaryFileChange.path) })
    : activityLabel(activity, (key, options) => t(key, options))

  return (
    <div
      data-tool-activity-row={activity.id}
      data-file-change-activity={fileChanges[0]?.changeId}
      className="group/row min-w-0"
    >
      <div className="flex min-w-0 items-center rounded-md transition-colors hover:bg-paper-hover">
        <button
          type="button"
          aria-expanded={hasDetails ? expanded : undefined}
          onClick={() => hasDetails && setExpanded((value) => !value)}
          className="flex min-h-6 min-w-0 flex-1 items-center gap-2 rounded-md px-1.5 text-left"
        >
          <LeadingIcon activity={activity} />
          <span className="min-w-0 truncate font-mono text-xs leading-5 text-ink-soft">
            {label}
          </span>
          {primaryFileChange && !expanded ? (
            <FileStats
              additions={primaryFileChange.additions}
              deletions={primaryFileChange.deletions}
              className="text-xs"
            />
          ) : null}
          {hasDetails ? (
            <ChevronRight
              size={13}
              className={`shrink-0 text-ink-faint transition-transform duration-200 ${
                expanded ? 'rotate-90' : ''
              }`}
            />
          ) : null}
        </button>
        {onOpenTrace ? (
          <button
            type="button"
            data-open-tool-trace={activity.id}
            aria-label={t('activity.openToolSpan')}
            title={t('activity.openToolSpan')}
            onClick={() => onOpenTrace(activity.id)}
            className="mr-1 grid size-6 shrink-0 place-items-center rounded-md text-ink-faint opacity-0 transition-opacity hover:bg-paper hover:text-clay focus-visible:opacity-100 group-hover/row:opacity-100"
          >
            <Activity size={12} />
          </button>
        ) : null}
      </div>

      <AnimatePresence initial={false}>
        {hasDetails && expanded ? (
          <motion.div
            key="details"
            initial={{ opacity: 0, height: 0 }}
            animate={{ opacity: 1, height: 'auto' }}
            exit={{ opacity: 0, height: 0 }}
            transition={{ duration: 0.16, ease: 'easeOut' }}
            className="overflow-hidden"
          >
            {fileChanges.length > 0 ? (
              <div data-file-change-details="true" className="space-y-2 py-1 pl-7">
                {fileChanges.map((change) => (
                  <div key={change.changeId} className="overflow-hidden rounded-xl border border-line">
                    <FileDiffPanel change={change} />
                  </div>
                ))}
              </div>
            ) : activity.name === 'bash' ? (
              <div className="py-1 pl-7">
                <ShellDetailsCard activity={activity} />
              </div>
            ) : (
              <div className="py-1 pl-7">
                <div className="overflow-hidden rounded-xl border border-line bg-paper">
                  {details.map((detail, index) => (
                    <div key={detail.label} className={index > 0 ? 'border-t border-line' : ''}>
                      <div className="px-3 pt-1.5 text-[11px] text-ink-faint">
                        {detail.label}
                      </div>
                      <pre
                        className={`max-h-72 overflow-auto whitespace-pre-wrap break-words px-3 pb-2 pt-0.5 font-mono text-xs leading-relaxed ${
                          detail.error ? 'text-status-danger-ink' : 'text-ink-soft'
                        }`}
                      >
                        {detail.value}
                      </pre>
                    </div>
                  ))}
                </div>
              </div>
            )}
          </motion.div>
        ) : null}
      </AnimatePresence>
    </div>
  )
}

function LeadingIcon({ activity }: { activity: ToolActivity }) {
  const { t } = useTranslation()
  const state = activity.state
  if (state === 'pending' || state === 'submitted' || state === 'running') {
    return (
      <span className="inline-flex shrink-0 items-center text-ink-faint" title={t('tool.running')}>
        <Loader2 size={14} className="animate-spin" />
        <span className="sr-only">{t('tool.running')}</span>
      </span>
    )
  }
  if (state === 'error') {
    return (
      <span className="inline-flex shrink-0 items-center text-status-danger" title={t('tool.error')}>
        <CircleAlert size={14} />
        <span className="sr-only">{t('tool.error')}</span>
      </span>
    )
  }
  if (state === 'denied' || state === 'interrupted') {
    return (
      <span className="inline-flex shrink-0 items-center text-ink-faint" title={t('tool.stopped')}>
        <CircleX size={14} />
        <span className="sr-only">{t('tool.stopped')}</span>
      </span>
    )
  }
  const Icon = TOOL_ICONS[activity.name] ?? Wrench
  return (
    <span className="inline-flex shrink-0 items-center text-ink-faint" title={t('tool.done')}>
      <Icon size={14} strokeWidth={1.9} />
      <span className="sr-only">{t('tool.done')}</span>
    </span>
  )
}

function ShellDetailsCard({ activity }: { activity: ToolActivity }) {
  const { t } = useTranslation()
  const command = activity.summary
  const output = activity.output
  const isError = activity.state === 'error'
  const [copied, setCopied] = useState(false)
  const copiedTimerRef = useRef<number | null>(null)

  useEffect(() => () => {
    if (copiedTimerRef.current !== null) window.clearTimeout(copiedTimerRef.current)
  }, [])

  async function copyTranscript() {
    const text = output ? `$ ${command}\n${output}` : `$ ${command}`
    try {
      await navigator.clipboard?.writeText(text)
    } catch {
      return
    }
    setCopied(true)
    if (copiedTimerRef.current !== null) window.clearTimeout(copiedTimerRef.current)
    copiedTimerRef.current = window.setTimeout(() => setCopied(false), 1_500)
  }

  return (
    <div className="overflow-hidden rounded-xl border border-line bg-paper">
      <div className="flex items-center gap-2 px-3 pt-1.5">
        <span className="min-w-0 flex-1 text-[11px] text-ink-faint">{t('tool.shell')}</span>
        <button
          type="button"
          aria-label={copied ? t('tool.copied') : t('tool.copy')}
          title={copied ? t('tool.copied') : t('tool.copy')}
          onClick={() => void copyTranscript()}
          className="grid size-6 shrink-0 place-items-center rounded-md text-ink-faint hover:bg-paper-hover hover:text-ink"
        >
          {copied ? <Check size={12} className="text-status-success" /> : <Copy size={12} />}
        </button>
      </div>
      <div className="max-h-72 overflow-auto px-3 pb-2 pt-0.5 font-mono text-xs leading-relaxed">
        {command ? (
          <div className="whitespace-pre-wrap break-words text-ink-soft">$ {command}</div>
        ) : null}
        {output ? (
          <div
            className={`whitespace-pre-wrap break-words ${
              isError ? 'text-status-danger-ink' : 'text-ink-faint'
            }`}
          >
            {output}
          </div>
        ) : null}
      </div>
    </div>
  )
}

function parseInput(input: string): Record<string, unknown> | null {
  if (!input) return null
  try {
    const parsed: unknown = JSON.parse(input)
    return parsed && typeof parsed === 'object' && !Array.isArray(parsed)
      ? (parsed as Record<string, unknown>)
      : null
  } catch {
    return null
  }
}

function summarize(toolName: string, input: Record<string, unknown> | null): string {
  if (!input) return ''
  if (toolName === 'bash' && typeof input.command === 'string') return input.command
  if (typeof input.path === 'string') return input.path
  if (typeof input.filePath === 'string') return input.filePath
  return ''
}

function fileName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path
}

function activityLabel(
  activity: ToolActivity,
  translate: (key: string, options?: Record<string, unknown>) => string,
): string {
  const target = activity.name === 'bash' ? activity.summary : fileName(activity.summary)
  switch (activity.name) {
    case 'bash':
      return target
        ? translate('tool.ranCommand', { command: target })
        : translate('tool.ranCommandFallback')
    case 'write':
      return target
        ? translate('tool.wroteFile', { name: target })
        : translate('tool.wroteFileFallback')
    case 'edit':
      return target
        ? translate('tool.editedFile', { name: target })
        : translate('tool.editedFileFallback')
    case 'read':
      return target
        ? translate('tool.readFile', { name: target })
        : translate('tool.readFileFallback')
    case 'list':
      return target
        ? translate('tool.listedDirectory', { name: target })
        : translate('tool.listedDirectoryFallback')
    default:
      return translate('tool.calledTool', { name: activity.name })
  }
}

function parseFileChange(artifact: ToolResultArtifact): FileChangeView | null {
  if (artifact.kind !== 'file_change' || !isRecord(artifact.payload)) return null
  const payload = artifact.payload
  if (
    typeof payload.changeId !== 'string' ||
    typeof payload.path !== 'string' ||
    (payload.kind !== 'created' && payload.kind !== 'modified') ||
    typeof payload.additions !== 'number' ||
    typeof payload.deletions !== 'number' ||
    typeof payload.afterHash !== 'string' ||
    !Array.isArray(payload.hunks)
  ) return null

  const hunks: FileDiffHunk[] = payload.hunks.flatMap((candidate) => {
    if (!isRecord(candidate) || !Array.isArray(candidate.lines)) return []
    if (
      typeof candidate.oldStart !== 'number' ||
      typeof candidate.oldLines !== 'number' ||
      typeof candidate.newStart !== 'number' ||
      typeof candidate.newLines !== 'number'
    ) return []
    const lines: FileDiffLine[] = candidate.lines.flatMap((line) => {
      if (!isRecord(line)) return []
      if (
        line.kind !== 'context' &&
        line.kind !== 'addition' &&
        line.kind !== 'deletion'
      ) return []
      if (typeof line.content !== 'string') return []
      return [{
        kind: line.kind,
        oldLine: typeof line.oldLine === 'number' ? line.oldLine : null,
        newLine: typeof line.newLine === 'number' ? line.newLine : null,
        content: line.content,
        noNewline: line.noNewline === true,
      }]
    })
    return [{
      oldStart: candidate.oldStart,
      oldLines: candidate.oldLines,
      newStart: candidate.newStart,
      newLines: candidate.newLines,
      lines,
    }]
  })

  return {
    changeId: payload.changeId,
    path: payload.path,
    kind: payload.kind,
    additions: payload.additions,
    deletions: payload.deletions,
    hunks,
    beforeHash: typeof payload.beforeHash === 'string' ? payload.beforeHash : null,
    afterHash: payload.afterHash,
    undone: payload.undone === true,
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function activityDetails(
  activity: ToolActivity,
  translate: (key: string) => string,
): Array<{
  label: string
  value: string
  error?: boolean
}> {
  const details: Array<{ label: string; value: string; error?: boolean }> = []
  const inputValue = (() => {
    if (activity.name === 'bash') return ''
    if (activity.name === 'write' && typeof activity.input?.content === 'string') {
      return activity.input.content
    }
    if (!activity.rawInput) return ''
    return activity.input ? JSON.stringify(activity.input, null, 2) : activity.rawInput
  })()
  if (inputValue) {
    details.push({ label: translate('tool.input'), value: inputValue })
  }
  if (activity.output) {
    details.push({
      label: translate('tool.output'),
      value: activity.output,
      error: activity.state === 'error',
    })
  }
  return details
}
