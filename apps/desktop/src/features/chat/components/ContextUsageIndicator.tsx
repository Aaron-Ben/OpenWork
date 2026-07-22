import { useId } from 'react'
import { useTranslation } from 'react-i18next'

import type { ContextUsage } from '../contextUsage'

interface ContextUsageIndicatorProps {
  usage?: ContextUsage | null
}

const RADIUS = 8
const CIRCUMFERENCE = 2 * Math.PI * RADIUS

export function ContextUsageIndicator({ usage }: ContextUsageIndicatorProps) {
  const { t } = useTranslation()
  const tooltipId = useId()
  const usedPercent = usage
    ? Math.min(100, Math.max(0, Math.round((usage.usedTokens / usage.totalTokens) * 100)))
    : 0
  const leftPercent = 100 - usedPercent
  const progressOffset = CIRCUMFERENCE * (1 - usedPercent / 100)
  const tone = !usage
    ? 'text-ink-faint'
    : usedPercent >= 90
      ? 'text-status-danger'
      : usedPercent >= 75
        ? 'text-status-warning-ink'
        : 'text-ink-faint'
  const barTone = usedPercent >= 90
    ? 'bg-status-danger'
    : usedPercent >= 75
      ? 'bg-status-warning'
      : 'bg-clay'
  const label = usage
    ? t('chat.contextUsageAria', {
        usedPercent,
        used: formatTokenCount(usage.usedTokens),
        total: formatTokenCount(usage.totalTokens),
      })
    : t('chat.contextUsageUnavailable')

  return (
    <div className="group relative shrink-0">
      <button
        type="button"
        className="flex h-8 items-center gap-1.5 rounded-lg px-1.5 text-ink-faint outline-none transition hover:bg-paper-hover hover:text-ink-soft focus-visible:ring-2 focus-visible:ring-clay/35"
        aria-label={label}
        aria-describedby={tooltipId}
      >
        <svg
          aria-hidden="true"
          viewBox="0 0 24 24"
          className={`size-[18px] -rotate-90 ${tone}`}
          data-context-usage-ring="true"
        >
          <circle
            cx="12"
            cy="12"
            r={RADIUS}
            fill="none"
            stroke="currentColor"
            strokeWidth="2.5"
            className="opacity-25"
          />
          <circle
            cx="12"
            cy="12"
            r={RADIUS}
            fill="none"
            stroke="currentColor"
            strokeWidth="2.5"
            strokeLinecap="round"
            strokeDasharray={CIRCUMFERENCE}
            strokeDashoffset={progressOffset}
            className="transition-[stroke-dashoffset] duration-300"
            data-context-usage-progress={usedPercent}
          />
        </svg>
        {usage ? (
          <span className={`font-mono text-[11px] tabular-nums leading-none ${tone}`}>
            {usedPercent}%
          </span>
        ) : null}
      </button>

      <div
        id={tooltipId}
        role="tooltip"
        className="pointer-events-none invisible absolute bottom-[calc(100%+10px)] left-1/2 z-50 min-w-[220px] -translate-x-1/2 translate-y-1 rounded-xl border border-line bg-paper px-3.5 py-2.5 opacity-0 shadow-[0_14px_36px_rgba(20,20,19,0.16)] transition-[opacity,transform,visibility] duration-150 group-hover:visible group-hover:translate-y-0 group-hover:opacity-100 group-focus-within:visible group-focus-within:translate-y-0 group-focus-within:opacity-100"
      >
        <div className="text-[11px] text-ink-faint">{t('chat.contextWindow')}</div>
        {usage ? (
          <>
            <div className="mt-1 text-sm font-medium text-ink">
              {t('chat.contextUsagePercent', { usedPercent, leftPercent })}
            </div>
            <div className="mt-2 h-1 overflow-hidden rounded-full bg-paper-hover">
              <div
                className={`h-full rounded-full transition-[width] duration-300 ${barTone}`}
                style={{ width: `${usedPercent}%` }}
              />
            </div>
            <div className="mt-1.5 text-xs text-ink-soft">
              {t('chat.contextTokensUsed', {
                used: formatTokenCount(usage.usedTokens),
                total: formatTokenCount(usage.totalTokens),
              })}
            </div>
            {usage.estimated ? (
              <div className="mt-1 text-[11px] text-ink-faint">{t('chat.contextUsageEstimated')}</div>
            ) : null}
          </>
        ) : (
          <div className="mt-1 text-xs text-ink-soft">
            {t('chat.contextUsageUnavailable')}
          </div>
        )}
        <span className="absolute -bottom-[5px] left-1/2 size-2 -translate-x-1/2 rotate-45 border-b border-r border-line bg-paper" />
      </div>
    </div>
  )
}

export function formatTokenCount(tokens: number): string {
  if (tokens < 1_000) return String(tokens)
  if (tokens < 1_000_000) return `${formatUnit(tokens / 1_000)}k`
  return `${formatUnit(tokens / 1_000_000)}m`
}

function formatUnit(value: number): string {
  return value >= 10 ? String(Math.round(value)) : String(Math.round(value * 10) / 10)
}
