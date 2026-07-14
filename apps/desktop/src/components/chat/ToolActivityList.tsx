import { memo, useMemo, useState } from 'react'
import { AnimatePresence, motion } from 'motion/react'
import { useTranslation } from 'react-i18next'
import {
  Check,
  ChevronDown,
  ChevronRight,
  CircleAlert,
  CircleX,
  FileText,
  Folder,
  Loader2,
  Pencil,
  Terminal,
  Wrench,
} from 'lucide-react'

import { extractText, type ContentBlock, type ToolResultState } from '../../type/parts'

type ActivityState = ToolResultState | 'pending' | 'submitted' | 'finished'

export interface ToolActivity {
  id: string
  name: string
  input: Record<string, unknown> | null
  rawInput: string
  summary: string
  output: string
  state: ActivityState
}

interface ToolActivityListProps {
  parts: ContentBlock[]
}

const TOOL_ICONS: Record<string, typeof Terminal> = {
  read: FileText,
  write: Pencil,
  list: Folder,
  bash: Terminal,
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
      })
    }
  }

  return order.flatMap((id) => {
    const activity = activities.get(id)
    return activity ? [activity] : []
  })
}

export const ToolActivityList = memo(function ToolActivityList({ parts }: ToolActivityListProps) {
  const { t } = useTranslation()
  const [expanded, setExpanded] = useState(true)
  const activities = useMemo(() => collectToolActivities(parts), [parts])

  if (activities.length === 0) return null

  const SummaryIcon = activities.some((activity) => activity.name === 'write') ? Pencil : Wrench
  const summary = activitySummary(activities, (key, options) => t(key, options))

  return (
    <div data-tool-activity-list="true" className="w-full py-1 text-sm text-ink-soft">
      <button
        type="button"
        data-tool-activity-summary="true"
        aria-expanded={expanded}
        onClick={() => setExpanded((value) => !value)}
        className="group/summary flex min-h-8 w-full items-center gap-2 rounded-md px-1.5 text-left transition-colors hover:bg-paper-hover"
      >
        <SummaryIcon size={16} className="shrink-0 text-ink-faint" strokeWidth={1.9} />
        <span className="min-w-0 flex-1 truncate text-sm text-ink-soft">{summary}</span>
        <ChevronDown
          size={15}
          className={`shrink-0 text-ink-faint transition-transform duration-200 ${
            expanded ? 'rotate-0' : '-rotate-90'
          }`}
        />
      </button>

      <AnimatePresence initial={false}>
        {expanded ? (
          <motion.div
            key="tool-activities"
            initial={{ opacity: 0, height: 0 }}
            animate={{ opacity: 1, height: 'auto' }}
            exit={{ opacity: 0, height: 0 }}
            transition={{ duration: 0.18, ease: 'easeOut' }}
            className="overflow-hidden"
          >
            <div className="space-y-0.5 pt-0.5">
              {activities.map((activity) => (
                <ToolActivityRow key={activity.id} activity={activity} />
              ))}
            </div>
          </motion.div>
        ) : null}
      </AnimatePresence>
    </div>
  )
})

function ToolActivityRow({ activity }: { activity: ToolActivity }) {
  const { t } = useTranslation()
  const [expanded, setExpanded] = useState(false)
  const Icon = TOOL_ICONS[activity.name] ?? Wrench
  const details = activityDetails(activity, (key) => t(key))
  const hasDetails = details.length > 0

  return (
    <div data-tool-activity-row={activity.id} className="min-w-0">
      <button
        type="button"
        aria-expanded={hasDetails ? expanded : undefined}
        onClick={() => hasDetails && setExpanded((value) => !value)}
        className="flex min-h-8 w-full items-center gap-2 rounded-md px-1.5 text-left transition-colors hover:bg-paper-hover"
      >
        <Icon size={16} className="shrink-0 text-ink-faint" strokeWidth={1.9} />
        <span className="min-w-0 flex-1 truncate text-sm text-ink-soft">
          {activityLabel(activity, (key, options) => t(key, options))}
        </span>
        <ActivityStatus state={activity.state} />
        {hasDetails ? (
          <ChevronRight
            size={14}
            className={`shrink-0 text-ink-faint transition-transform duration-200 ${
              expanded ? 'rotate-90' : ''
            }`}
          />
        ) : null}
      </button>

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
            <div className="ml-6 border-l border-line py-1 pl-3">
              {details.map((detail) => (
                <div key={detail.label} className="mb-2 last:mb-0">
                  <div className="mb-1 text-[10px] uppercase tracking-wider text-ink-faint">
                    {detail.label}
                  </div>
                  <pre
                    className={`max-h-72 overflow-auto whitespace-pre-wrap break-words rounded-md px-3 py-2 font-mono text-[11px] leading-relaxed ${
                      detail.error ? 'bg-red-50 text-red-700' : 'bg-paper-hover text-ink-soft'
                    }`}
                  >
                    {detail.value}
                  </pre>
                </div>
              ))}
            </div>
          </motion.div>
        ) : null}
      </AnimatePresence>
    </div>
  )
}

function ActivityStatus({ state }: { state: ActivityState }) {
  const { t } = useTranslation()
  if (state === 'pending' || state === 'submitted' || state === 'running') {
    return (
      <span className="inline-flex shrink-0 items-center text-ink-faint" title={t('tool.running')}>
        <Loader2 size={13} className="animate-spin" />
        <span className="sr-only">{t('tool.running')}</span>
      </span>
    )
  }
  if (state === 'error') {
    return (
      <span className="inline-flex shrink-0 items-center text-red-500" title={t('tool.error')}>
        <CircleAlert size={13} />
        <span className="sr-only">{t('tool.error')}</span>
      </span>
    )
  }
  if (state === 'denied' || state === 'interrupted') {
    return (
      <span className="inline-flex shrink-0 items-center text-ink-faint" title={t('tool.stopped')}>
        <CircleX size={13} />
        <span className="sr-only">{t('tool.stopped')}</span>
      </span>
    )
  }
  return (
    <span className="inline-flex shrink-0 items-center text-emerald-600" title={t('tool.done')}>
      <Check size={13} />
      <span className="sr-only">{t('tool.done')}</span>
    </span>
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
  return ''
}

function fileName(path: string): string {
  return path.split('/').filter(Boolean).pop() ?? path
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

function activitySummary(
  activities: ToolActivity[],
  translate: (key: string, options?: Record<string, unknown>) => string,
): string {
  const categories: string[] = []
  const seen = new Set<string>()
  for (const activity of activities) {
    const category = ['write', 'read', 'list', 'bash'].includes(activity.name)
      ? activity.name
      : `other:${activity.name}`
    if (seen.has(category)) continue
    seen.add(category)
    switch (activity.name) {
      case 'write':
        categories.push(translate('tool.editedFiles'))
        break
      case 'read':
        categories.push(translate('tool.readFiles'))
        break
      case 'list':
        categories.push(translate('tool.listedDirectories'))
        break
      case 'bash':
        categories.push(translate('tool.ranCommands'))
        break
      default:
        categories.push(translate('tool.calledTool', { name: activity.name }))
    }
  }
  return categories.join(translate('tool.summarySeparator'))
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
