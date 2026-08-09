import { memo, useMemo, useState } from 'react'
import { AnimatePresence, motion } from 'motion/react'
import { useTranslation } from 'react-i18next'
import {
  Activity,
  ChevronRight,
  CircleAlert,
  CircleX,
  Loader2,
  Wrench,
} from 'lucide-react'

import {
  extractText,
  type ContentBlock,
  type ToolResultArtifact,
} from '@/types/parts'
import { isFailure, type ToolActivity } from '../toolActivity'
import { FileChangeCard } from './FileChangeCard'
import {
  isReadonlyDisplayTool,
  ReadonlyToolActivityRow,
} from './ReadonlyToolActivity'
import {
  FileChangeActivityRow,
  isFileChangeDisplayTool,
  type FileChangeView,
  type FileDiffHunk,
  type FileDiffLine,
} from './FileDiffPanel'
import {
  bashActivityFailed,
  BashToolActivityRow,
  isBashDisplayTool,
} from './BashToolActivity'

interface ToolActivityListProps {
  parts: ContentBlock[]
  turnActive?: boolean
  onOpenTrace?: (providerToolCallId: string) => void
  onUndoFileChanges?: (changeIds: string[]) => Promise<void>
  onReapplyFileChanges?: (changeIds: string[]) => Promise<void>
  onReviewFileChanges?: (changes: FileChangeView[]) => void
  fileChangePresentation?: 'activity' | 'summary'
  workspaceRoot?: string
}

export function collectToolActivities(parts: ContentBlock[]): ToolActivity[] {
  const order: string[] = []
  const activities = new Map<string, ToolActivity>()
  let sequence = 0
  let separatedBefore = false

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
        output: existing?.output ?? '',
        state: existing?.state ?? part.state,
        artifacts: existing?.artifacts ?? [],
        sequence: existing?.sequence ?? sequence++,
        separatedBefore: existing?.separatedBefore ?? separatedBefore,
      })
      separatedBefore = false
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
        output: extractText(part.output),
        state: part.state,
        artifacts: part.artifacts ?? [],
        sequence: existing?.sequence ?? sequence++,
        separatedBefore: existing?.separatedBefore ?? separatedBefore,
      })
      if (!existing) separatedBefore = false
      continue
    }

    // 模型文本或 thinking 位于两次工具调用之间时，下一次调用必须开启新组。
    separatedBefore = true
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
  turnActive = false,
  onOpenTrace,
  onUndoFileChanges,
  onReapplyFileChanges,
  onReviewFileChanges,
  fileChangePresentation = 'activity',
  workspaceRoot,
}: ToolActivityListProps) {
  const [toolExpansion, setToolExpansion] = useState<Record<string, boolean>>({})
  const allActivities = useMemo(
    () => collectToolActivities(parts).filter((activity) => activity.name !== 'update_plan'),
    [parts],
  )
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
  const activityGroups = useMemo(() => groupActivities(activities), [activities])

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
          workspaceRoot={workspaceRoot}
        />
      ) : null}
      {activities.length > 0 ? (
        <div className="space-y-0.5">
          {activityGroups.map((group) => {
            const rememberedExpansion = group.activities
              .map((activity) => toolExpansion[activity.id])
              .find((value) => value != null)
            const rememberExpansion = (expanded: boolean) => setToolExpansion((current) => ({
              ...current,
              ...Object.fromEntries(group.activities.map((activity) => [activity.id, expanded])),
            }))
            if (isReadonlyDisplayTool(group.activities[0].name)) return (
              <ReadonlyToolActivityRow
                key={group.id}
                activities={group.activities}
                expanded={rememberedExpansion}
                onExpandedChange={rememberExpansion}
                onOpenTrace={onOpenTrace}
              />
            )
            if (isFileChangeDisplayTool(group.activities[0].name)) return (
              <FileChangeActivityRow
                key={group.id}
                activities={group.activities}
                activityChanges={group.activities.map((activity) => collectFileChanges([activity]))}
                turnActive={turnActive}
                expanded={rememberedExpansion}
                onExpandedChange={rememberExpansion}
                onOpenTrace={onOpenTrace}
                onUndoFileChanges={onUndoFileChanges}
                onReapplyFileChanges={onReapplyFileChanges}
                workspaceRoot={workspaceRoot}
              />
            )
            if (isBashDisplayTool(group.activities[0].name)) return (
              <BashToolActivityRow
                key={group.id}
                activities={group.activities}
                expanded={rememberedExpansion}
                onExpandedChange={rememberExpansion}
                onOpenTrace={onOpenTrace}
              />
            )
            return (
              <ToolActivityRow
                key={group.id}
                activity={group.activities[0]}
                onOpenTrace={onOpenTrace}
              />
            )
          })}
        </div>
      ) : null}
    </div>
  )
})

interface ToolActivityGroup {
  id: string
  activities: ToolActivity[]
}

function groupActivities(activities: ToolActivity[]): ToolActivityGroup[] {
  const groups: ToolActivityGroup[] = []
  for (const activity of activities) {
    const previousGroup = groups[groups.length - 1]
    const previous = previousGroup?.activities[previousGroup.activities.length - 1]
    const canGroup = previousGroup
      && previous
      && isGroupedDisplayTool(activity.name)
      && activity.name === previous.name
      && !isDisplayFailure(activity)
      && !isDisplayFailure(previous)
      && !activity.separatedBefore
      && activity.sequence === previous.sequence + 1
    if (canGroup) {
      previousGroup.activities.push(activity)
    } else {
      groups.push({ id: activity.id, activities: [activity] })
    }
  }
  return groups
}

function isGroupedDisplayTool(name: string): boolean {
  return isReadonlyDisplayTool(name) || isFileChangeDisplayTool(name) || isBashDisplayTool(name)
}

function isDisplayFailure(activity: ToolActivity): boolean {
  return isFailure(activity)
    || activity.name === 'bash' && bashActivityFailed(activity)
}

function ToolActivityRow({
  activity,
  onOpenTrace,
}: {
  activity: ToolActivity
  onOpenTrace?: (providerToolCallId: string) => void
}) {
  const { t } = useTranslation()
  const [expanded, setExpanded] = useState(false)
  const details = activityDetails(activity, (key) => t(key))
  const hasDetails = details.length > 0

  return (
    <div
      data-tool-activity-row={activity.id}
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
            {t('tool.calledTool', { name: activity.name })}
          </span>
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
  // 成功态图标是装饰：动作名就在紧邻的文字里，读屏再念一遍只是噪音。
  return <Wrench size={14} strokeWidth={1.9} aria-hidden="true" className="shrink-0 text-ink-faint" />
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
  const inputValue = activity.rawInput
    ? activity.input ? JSON.stringify(activity.input, null, 2) : activity.rawInput
    : ''
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
