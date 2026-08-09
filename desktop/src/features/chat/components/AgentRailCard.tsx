import type { TFunction } from 'i18next'
import { useTranslation } from 'react-i18next'

import orchestratorAvatar from '@/assets/agents/orchestrator.svg'
import subAgentOrbitAvatar from '@/assets/agents/subagent-orbit.svg'
import subAgentPinwheelAvatar from '@/assets/agents/subagent-pinwheel.svg'
import subAgentPrismAvatar from '@/assets/agents/subagent-prism.svg'
import subAgentRippleAvatar from '@/assets/agents/subagent-ripple.svg'

import { formatTokenCount } from '../agentPresentation'
import type { AgentRailItem } from '../agentRailModel'

const CHILD_AGENT_AVATARS = [
  subAgentOrbitAvatar,
  subAgentPrismAvatar,
  subAgentPinwheelAvatar,
  subAgentRippleAvatar,
] as const

const childAvatarBySession = new Map<string, string>()

function childAvatar(sessionId: string): string {
  const assigned = childAvatarBySession.get(sessionId)
  if (assigned) return assigned

  // 随机只发生在子会话第一次出现时，避免状态和 token 更新让图案不断跳变。
  const avatar = CHILD_AGENT_AVATARS[Math.floor(Math.random() * CHILD_AGENT_AVATARS.length)]
  childAvatarBySession.set(sessionId, avatar)
  return avatar
}

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
  tokenScale,
}: {
  item: AgentRailItem
  selected: boolean
  onSelect: (sessionId: string) => void
  tokenScale?: number
}) {
  const { t } = useTranslation()
  const statusLabel = item.status === 'idle' || item.status === 'completed'
    ? t('chat.agents.standby')
    : t(`chat.subAgents.status.${item.status}`)
  const duration = formatAgentDuration(item.elapsedMs, t)
  const durationLabel = duration
    ? t(item.status === 'running' ? 'chat.agents.runningFor' : 'chat.agents.lastRun', { duration })
    : '--:--'
  const accessibleDetails = [item.role, statusLabel, durationLabel].filter(Boolean).join(' · ')

  if (!item.orchestrator) {
    const width = `${Math.min(100, Math.max(0, (tokenScale ?? (item.tokens > 0 ? 1 : 0)) * 100))}%`
    return (
      <button
        type="button"
        data-agent-card={item.sessionId}
        data-agent-status={item.status}
        aria-current={selected ? 'true' : undefined}
        aria-label={`${t('chat.agents.open', { name: item.task })} · ${accessibleDetails}`}
        className={`w-full rounded-xl border px-3 py-2.5 text-left transition ${
          selected
            ? 'border-clay bg-clay-soft'
            : 'border-transparent bg-paper hover:border-line-strong'
        }`}
        onClick={() => onSelect(item.sessionId)}
      >
        <span className="flex min-w-0 items-center gap-2.5">
          <img
            src={childAvatar(item.sessionId)}
            alt=""
            aria-hidden="true"
            title={item.role}
            data-agent-avatar="subagent"
            className="size-6 shrink-0"
          />
          <span className="min-w-0 flex-1 truncate text-xs font-semibold text-ink" title={item.task}>
            {item.task}
          </span>
          <span className="shrink-0 font-mono text-[11px] tabular-nums text-ink-faint">
            {formatTokenCount(item.tokens)}
          </span>
        </span>
        <span
          data-agent-card-meta="true"
          className="mt-1.5 flex min-w-0 items-center gap-1.5 text-[11px]"
        >
          <span aria-hidden="true" className={`size-1.5 shrink-0 rounded-full ${statusDotClass(item)}`} />
          <span className={`shrink-0 ${statusToneClass(item)}`}>{statusLabel}</span>
          <span className="truncate text-ink-faint">· {durationLabel}</span>
        </span>
        <span aria-hidden="true" className="mt-1.5 block h-1 overflow-hidden rounded-full bg-line">
          <span
            data-agent-token-scale={item.sessionId}
            className={`block h-full rounded-full ${tokenToneClass(item)}`}
            style={{ width }}
          />
        </span>
      </button>
    )
  }

  return (
    <button
      type="button"
      data-agent-card={item.sessionId}
      data-agent-status={item.status}
      data-agent-orchestrator-card="true"
      aria-current={selected ? 'true' : undefined}
      aria-label={`${t('chat.agents.open', { name: item.role })} · ${accessibleDetails}`}
      className={`w-full rounded-xl border p-3 text-left shadow-sm transition ${
        selected
          ? 'border-clay bg-clay-soft'
          : 'border-clay/55 bg-paper hover:border-clay'
      }`}
      onClick={() => onSelect(item.sessionId)}
    >
      <div className="flex items-start gap-2.5">
        <img
          src={orchestratorAvatar}
          alt=""
          aria-hidden="true"
          data-agent-avatar="orchestrator"
          className="size-7 shrink-0"
        />
        <span className="min-w-0 flex-1">
          <span className="flex items-center gap-2">
            <span className="min-w-0 flex-1 truncate font-sans text-sm font-medium text-ink">
              {item.role}
            </span>
            <span className="shrink-0 rounded-full bg-clay-soft px-2 py-0.5 font-mono text-[11px] font-semibold tabular-nums text-clay">
              {formatTokenCount(item.tokens)}
            </span>
          </span>
          <span className="mt-1.5 block truncate text-xs text-ink-soft" title={item.task}>{item.task}</span>
          <span className="mt-1.5 flex min-w-0 items-center gap-1.5 text-[11px]">
            <span aria-hidden="true" className={`size-1.5 shrink-0 rounded-full ${statusDotClass(item)}`} />
            <span className={`shrink-0 ${statusToneClass(item)}`}>{statusLabel}</span>
            <span className="truncate text-ink-faint">· {durationLabel}</span>
          </span>
        </span>
      </div>
    </button>
  )
}

function statusDotClass(item: AgentRailItem): string {
  if (item.status === 'running') return 'animate-pulse bg-clay'
  if (item.status === 'failed' || item.status === 'cancelled') return 'bg-status-danger'
  return 'bg-status-success'
}

function tokenToneClass(item: AgentRailItem): string {
  if (item.status === 'running') return 'bg-clay'
  if (item.status === 'failed' || item.status === 'cancelled') return 'bg-status-danger'
  return 'bg-status-success/60'
}

function formatAgentDuration(durationMs: number | null, t: TFunction): string | null {
  if (durationMs == null || !Number.isFinite(durationMs) || durationMs < 0) return null
  const totalSeconds = Math.floor(durationMs / 1000)
  const seconds = totalSeconds % 60
  const totalMinutes = Math.floor(totalSeconds / 60)
  if (totalMinutes < 60) {
    return totalMinutes > 0
      ? t('chat.agents.durationMinutesSeconds', { minutes: totalMinutes, seconds })
      : t('chat.agents.durationSeconds', { seconds })
  }
  return t('chat.agents.durationHoursMinutes', {
    hours: Math.floor(totalMinutes / 60),
    minutes: totalMinutes % 60,
  })
}
