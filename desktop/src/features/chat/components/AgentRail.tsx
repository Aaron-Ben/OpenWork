import { useTranslation } from 'react-i18next'

import { formatElapsedClock, formatTokenCount } from '../agentPresentation'
import {
  agentRailCounts,
  agentRailElapsedMs,
  agentRailTotalTokens,
  type AgentRailItem,
} from '../agentRailModel'
import { AgentRailCard } from './AgentRailCard'

interface AgentRailProps {
  items: readonly AgentRailItem[]
  /** 当前在中栏展开的子智能体；主控视图下为 null。 */
  selectedSessionId: string | null
  error: string | null
  onSelect: (sessionId: string) => void
}

export function AgentRail({ items, selectedSessionId, error, onSelect }: AgentRailProps) {
  const { t } = useTranslation()
  const counts = agentRailCounts(items)
  const totalTokens = agentRailTotalTokens(items)
  const elapsedMs = agentRailElapsedMs(items)

  return (
    <aside
      data-agent-rail="true"
      aria-label={t('chat.agents.panel')}
      className="flex h-full w-[300px] shrink-0 flex-col overflow-hidden border-l border-line bg-paper-hover"
    >
      <div className="shrink-0 px-4 pb-2 pt-4">
        <div className="flex items-center gap-2">
          <h2 className="min-w-0 flex-1 truncate font-sans text-sm font-semibold text-ink">
            {t('chat.agents.title')}
          </h2>
          <span className="shrink-0 rounded-full bg-paper px-2 py-0.5 font-mono text-[11px] tabular-nums text-ink-faint">
            {formatElapsedClock(elapsedMs)}
          </span>
        </div>
        <p className="mt-1 truncate text-xs text-ink-faint">
          {[
            t('chat.agents.runningCount', { count: counts.running }),
            t('chat.agents.standbyCount', { count: counts.standby }),
            t('chat.agents.hint'),
          ].join(' · ')}
        </p>
      </div>

      {error ? (
        <p className="px-4 py-2 text-xs text-status-danger-ink" role="alert">
          {t('chat.subAgents.listFailed', { reason: error })}
        </p>
      ) : null}

      <div className="min-h-0 flex-1 overflow-y-auto px-3 pb-3">
        <div className="grid gap-2">
          {items.map((item) => (
            <AgentRailCard
              key={item.sessionId}
              item={item}
              selected={selectedSessionId === item.sessionId}
              onSelect={onSelect}
            />
          ))}
        </div>
      </div>

      <div
        data-agent-rail-footer="true"
        className="flex shrink-0 items-center justify-between border-t border-line px-4 py-2.5 text-xs text-ink-faint"
      >
        <span>{t('chat.agents.total')}</span>
        <span className="font-mono tabular-nums">
          {formatTokenCount(totalTokens)} {t('chat.agents.tokenUnit')}
        </span>
      </div>
    </aside>
  )
}
