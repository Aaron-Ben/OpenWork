import { useTranslation } from 'react-i18next'

import { agentInitial, formatElapsedClock, formatTokenCount } from '../agentPresentation'
import type { AgentRailItem } from '../agentRailModel'

/*
  状态色一律走 status-* / clay 语义 token，暗色下自动跟随。
  这里只表达"什么状态"，不表达"完成了多少" —— 运行时没有进度这种量。
*/
function statusToneClass(item: AgentRailItem): string {
  if (item.status === 'running') return 'text-clay'
  if (item.status === 'completed') return 'text-status-success-ink'
  if (item.status === 'failed' || item.status === 'cancelled') return 'text-status-danger-ink'
  return 'text-ink-faint'
}

export function AgentRailCard({
  item,
  selected,
  onSelect,
}: {
  item: AgentRailItem
  selected: boolean
  onSelect: (sessionId: string) => void
}) {
  const { t } = useTranslation()
  const activity = item.toolActivity
  return (
    <button
      type="button"
      data-agent-card={item.sessionId}
      data-agent-status={item.status}
      aria-current={selected ? 'true' : undefined}
      aria-label={t('chat.agents.open', { name: item.role })}
      className={`w-full rounded-xl border p-3 text-left transition ${
        selected
          ? 'border-clay bg-clay-soft'
          : 'border-line bg-paper hover:border-line-strong'
      }`}
      onClick={() => onSelect(item.sessionId)}
    >
      <div className="flex items-start gap-2.5">
        <span
          aria-hidden="true"
          className={`grid size-7 shrink-0 place-items-center rounded-full text-xs font-semibold ${
            item.orchestrator ? 'bg-clay text-paper' : 'bg-clay-soft text-clay'
          }`}
        >
          {agentInitial(item.role)}
        </span>
        <span className="min-w-0 flex-1">
          <span className="flex items-baseline gap-2">
            <span className="min-w-0 flex-1 truncate font-sans text-sm font-medium text-ink">
              {item.role}
            </span>
            <span className="shrink-0 font-mono text-[11px] tabular-nums text-ink-faint">
              {formatTokenCount(item.tokens)} {t('chat.agents.tokenUnit')}
            </span>
          </span>
          <span className="mt-0.5 flex items-center gap-1.5">
            {item.status === 'running' ? (
              <span aria-hidden="true" className="size-1.5 shrink-0 animate-pulse rounded-full bg-clay" />
            ) : null}
            <span className={`min-w-0 truncate text-xs ${statusToneClass(item)}`}>
              {t(`chat.subAgents.status.${item.status}`)}
            </span>
            <span className="shrink-0 font-mono text-[11px] tabular-nums text-ink-faint">
              {formatElapsedClock(item.elapsedMs)}
            </span>
          </span>
          <span className="mt-1 block truncate text-xs text-ink-faint">{item.task}</span>
          {activity ? (
            <span className="mt-1 block truncate font-mono text-[11px] text-ink-faint">
              {t('chat.agents.toolAttempt', { name: activity.name, count: activity.attempt })}
            </span>
          ) : null}
        </span>
      </div>
    </button>
  )
}
