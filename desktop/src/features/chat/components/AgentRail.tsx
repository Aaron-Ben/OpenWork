import { useTranslation } from 'react-i18next'

import { formatTokenCount } from '../agentPresentation'
import {
  agentRailCounts,
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
  const orchestrator = items.find((item) => item.orchestrator) ?? null
  const children = items.filter((item) => !item.orchestrator)
  const maxChildTokens = Math.max(1, ...children.map((item) => item.tokens))

  return (
    <aside
      data-agent-rail="true"
      aria-label={t('chat.agents.panel')}
      className="flex h-full w-[300px] shrink-0 flex-col overflow-hidden border-l border-line bg-paper-hover"
    >
      <div className="shrink-0 px-4 pb-3 pt-4">
        <div className="flex items-center gap-2">
          <h2 className="min-w-0 flex-1 truncate font-sans text-sm font-semibold text-ink">
            {t('chat.agents.title')}
          </h2>
          <span
            data-agent-rail-total="true"
            className="shrink-0 font-mono text-[11px] tabular-nums text-ink-faint"
          >
            {t('chat.agents.totalTokens', {
              tokens: formatTokenCount(totalTokens),
              unit: t('chat.agents.tokenUnit'),
            })}
          </span>
        </div>
        <p className="mt-1 truncate text-xs text-ink-faint">
          {[
            t('chat.agents.runningCount', { count: counts.running }),
            t('chat.agents.standbyCount', { count: counts.standby }),
          ].join(' · ')}
        </p>
      </div>

      {error ? (
        <p className="px-4 py-2 text-xs text-status-danger-ink" role="alert">
          {t('chat.subAgents.listFailed', { reason: error })}
        </p>
      ) : null}

      <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-3">
        {orchestrator ? (
          <div data-agent-orchestrator="true">
            <AgentRailCard
              item={orchestrator}
              selected={selectedSessionId === orchestrator.sessionId}
              onSelect={onSelect}
            />
          </div>
        ) : null}

        <div className="mb-2 mt-4 px-2 text-[11px] font-medium text-ink-faint">
          {t('chat.agents.childCount', { count: children.length })}
        </div>
        <div data-agent-children="true" className="grid gap-2">
          {children.map((item) => (
            <AgentRailCard
              key={item.sessionId}
              item={item}
              selected={selectedSessionId === item.sessionId}
              tokenScale={item.tokens / maxChildTokens}
              onSelect={onSelect}
            />
          ))}
        </div>
      </div>
    </aside>
  )
}
