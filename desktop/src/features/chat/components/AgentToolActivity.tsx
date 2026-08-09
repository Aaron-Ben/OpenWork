import { Check, CircleAlert, Loader2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { isFailure, isInProgress, type ToolActivity } from '../toolActivity'
import { Separator, ToolActivityFrame } from './ToolActivityFrame'

type AgentDisplayTool = 'spawn_agent' | 'wait_agent'

interface AgentToolActivityRowProps {
  activities: ToolActivity[]
  expanded?: boolean
  onExpandedChange: (expanded: boolean) => void
  onOpenTrace?: (providerToolCallId: string) => void
}

export function isAgentDisplayTool(name: string): name is AgentDisplayTool {
  return name === 'spawn_agent' || name === 'wait_agent'
}

export function AgentToolActivityRow({
  activities,
  expanded: rememberedExpanded,
  onExpandedChange,
  onOpenTrace,
}: AgentToolActivityRowProps) {
  const { t } = useTranslation()
  const primary = activities[0]
  if (!primary || !isAgentDisplayTool(primary.name)) return null

  const failed = activities.some((activity) => isFailure(activity))
  const running = activities.some((activity) => isInProgress(activity))
  const expanded = failed || (rememberedExpanded ?? primary.name === 'spawn_agent')
  const failureText = activities.map((activity) => activity.output).join('\n').toLowerCase()
  const failureLabel = primary.name === 'wait_agent' && failureText.includes('doom loop')
    ? t('tool.agentActivity.waitLimitExceeded')
    : t('tool.agentActivity.failed')
  const summary = (
    <>
      <span className="shrink-0 font-mono text-xs font-semibold text-ink">
        {primary.name}
      </span>
      {activities.length > 1 ? (
        <span className="shrink-0 rounded-full bg-paper-hover px-1.5 py-0.5 font-mono text-[10px] text-ink-faint">
          ×{activities.length}
        </span>
      ) : null}
      <Separator />
      <span className={`min-w-0 truncate text-xs ${failed ? 'font-medium text-status-danger-ink' : 'text-ink-faint'}`}>
        {failed
          ? failureLabel
          : t(primary.name === 'spawn_agent'
              ? 'tool.agentActivity.spawnedExplorer'
              : 'tool.agentActivity.waiting')}
      </span>
    </>
  )

  return (
    <div data-agent-tool-activity={primary.name}>
      <ToolActivityFrame
        toolCallId={primary.id}
        tier={failed ? 'failure' : 'readonly'}
        expanded={expanded}
        onExpandedChange={failed ? undefined : onExpandedChange}
        statusIcon={<AgentToolStatusIcon failed={failed} running={running} />}
        summary={summary}
        onOpenTrace={onOpenTrace}
        detailsMaxHeightClass="max-h-72"
      >
        {primary.name === 'spawn_agent' ? (
          <SpawnAgentDetails activities={activities} />
        ) : (
          <WaitAgentDetails activities={activities} />
        )}
        {failed && onOpenTrace ? (
          <div className="border-t border-status-danger-border bg-paper px-3 py-2">
            <button
              type="button"
              onClick={() => onOpenTrace(primary.id)}
              className="rounded-full border border-status-danger-border px-3 py-1 text-xs font-medium text-status-danger-ink transition-colors hover:bg-status-danger-soft"
            >
              {t('tool.agentActivity.inspectFailure')}
            </button>
          </div>
        ) : null}
      </ToolActivityFrame>
    </div>
  )
}

function AgentToolStatusIcon({ failed, running }: { failed: boolean; running: boolean }) {
  const { t } = useTranslation()
  if (running) return (
    <span className="grid size-5 shrink-0 place-items-center rounded-full bg-paper-hover text-ink-faint" title={t('tool.running')}>
      <Loader2 size={12} className="animate-spin" />
    </span>
  )
  if (failed) return (
    <span className="grid size-5 shrink-0 place-items-center rounded-full bg-clay text-paper" title={t('tool.error')}>
      <CircleAlert size={12} />
    </span>
  )
  return (
    <span className="grid size-5 shrink-0 place-items-center rounded-full bg-status-success-soft text-status-success-ink">
      <Check size={12} aria-hidden="true" />
    </span>
  )
}

function SpawnAgentDetails({ activities }: { activities: ToolActivity[] }) {
  return (
    <div className="divide-y divide-line/70 bg-paper">
      {activities.map((activity) => {
        const taskName = stringInput(activity, 'task_name') || activity.id
        return (
          <div
            key={activity.id}
            data-agent-spawn-task={taskName}
            className="flex min-h-9 min-w-0 items-center gap-3 px-11 py-1.5"
          >
            <span className="min-w-0 flex-1 truncate font-mono text-xs font-medium text-ink-soft" title={taskName}>
              {taskName}
            </span>
            {isFailure(activity) && activity.output ? (
              <span className="max-w-[55%] truncate text-xs text-status-danger-ink" title={activity.output}>
                {activity.output}
              </span>
            ) : null}
          </div>
        )
      })}
    </div>
  )
}

function WaitAgentDetails({ activities }: { activities: ToolActivity[] }) {
  const { t } = useTranslation()
  return (
    <div className="divide-y divide-line/70 bg-paper">
      {activities.map((activity, index) => {
        const result = parseWaitResult(activity.output)
        const state = isInProgress(activity)
          ? t('tool.running')
          : isFailure(activity)
            ? activity.output || t('tool.agentActivity.failed')
            : result?.timed_out
              ? t('tool.agentActivity.timedOut')
              : result?.delivered
                ? t('tool.agentActivity.delivered')
                : t('tool.agentActivity.completed')
        return (
          <div key={activity.id} className="flex min-h-9 items-center gap-3 px-11 py-1.5 text-xs">
            <span className="font-mono text-ink-faint">#{index + 1}</span>
            <span className={`min-w-0 flex-1 truncate ${isFailure(activity) ? 'text-status-danger-ink' : 'text-ink-soft'}`} title={state}>
              {state}
            </span>
          </div>
        )
      })}
    </div>
  )
}

function stringInput(activity: ToolActivity, key: string): string {
  const value = activity.input?.[key]
  return typeof value === 'string' ? value : ''
}

function parseWaitResult(output: string): { delivered?: boolean; timed_out?: boolean } | null {
  if (!output) return null
  try {
    const parsed: unknown = JSON.parse(output)
    return parsed && typeof parsed === 'object' && !Array.isArray(parsed)
      ? parsed as { delivered?: boolean; timed_out?: boolean }
      : null
  } catch {
    return null
  }
}
