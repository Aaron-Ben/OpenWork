import { useEffect, useRef, useState } from 'react'
import { ChevronRight } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import type { ContextUsage, ContextUsageBreakdown } from '../contextUsage'

interface ContextUsageIndicatorProps {
  usage?: ContextUsage | null
  breakdown?: ContextUsageBreakdown | null
  inspectorOpen?: boolean
  onInspect?: () => void
}

const RADIUS = 8
const CIRCUMFERENCE = 2 * Math.PI * RADIUS

export function ContextUsageIndicator({
  usage,
  breakdown,
  inspectorOpen = false,
  onInspect,
}: ContextUsageIndicatorProps) {
  const { t } = useTranslation()
  const [panelOpen, setPanelOpen] = useState(false)
  const rootRef = useRef<HTMLDivElement>(null)
  const panelVisible = panelOpen && !inspectorOpen

  useEffect(() => {
    if (!panelVisible) return
    function handlePointerDown(event: MouseEvent) {
      if (!rootRef.current?.contains(event.target as Node)) setPanelOpen(false)
    }
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === 'Escape') setPanelOpen(false)
    }
    document.addEventListener('mousedown', handlePointerDown)
    document.addEventListener('keydown', handleKeyDown)
    return () => {
      document.removeEventListener('mousedown', handlePointerDown)
      document.removeEventListener('keydown', handleKeyDown)
    }
  }, [panelVisible])

  const usedPercent = usage ? usagePercent(usage) : 0
  const label = usage
    ? t('chat.contextUsageAria', {
        usedPercent,
        used: formatTokenCount(usage.usedTokens),
        total: formatTokenCount(usage.totalTokens),
      })
    : t('chat.contextUsageUnavailable')

  return (
    <div ref={rootRef} className="relative shrink-0">
      <button
        type="button"
        className="flex h-8 items-center gap-1.5 rounded-lg px-1.5 text-ink-faint outline-none transition hover:bg-paper-hover hover:text-ink-soft focus-visible:ring-2 focus-visible:ring-clay/35"
        aria-label={label}
        aria-haspopup="dialog"
        aria-expanded={panelVisible}
        onClick={() => setPanelOpen((open) => !open)}
      >
        <svg
          aria-hidden="true"
          viewBox="0 0 24 24"
          className="size-[18px] -rotate-90 text-ink-faint"
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
            strokeDashoffset={CIRCUMFERENCE * (1 - usedPercent / 100)}
            className="text-clay transition-[stroke-dashoffset] duration-300"
            data-context-usage-progress={usedPercent}
          />
        </svg>
        {usage ? (
          <span className="font-mono text-[11px] tabular-nums leading-none text-ink-faint">
            {usedPercent}%
          </span>
        ) : null}
      </button>

      {panelVisible ? (
        <ContextUsagePanel
          usage={usage ?? null}
          breakdown={breakdown ?? null}
          onShowDetails={onInspect ? () => { setPanelOpen(false); onInspect() } : undefined}
        />
      ) : null}
    </div>
  )
}

interface ContextUsagePanelProps {
  usage: ContextUsage | null
  breakdown: ContextUsageBreakdown | null
  onShowDetails?: () => void
  /** Initial expanded state; used by static-markup tests that cannot click. */
  defaultExpanded?: boolean
}

export function ContextUsagePanel({ usage, breakdown, onShowDetails, defaultExpanded = false }: ContextUsagePanelProps) {
  const { t } = useTranslation()
  const [expanded, setExpanded] = useState(defaultExpanded)
  const usedPercent = usage ? usagePercent(usage) : 0
  const barTone = usedPercent >= 90
    ? 'bg-status-danger'
    : usedPercent >= 75
      ? 'bg-status-warning'
      : 'bg-clay'
  const categories = breakdown
    ? [
        { key: 'messages', label: t('chat.contextPanel.messages'), tokens: breakdown.messagesTokens, dot: 'bg-clay' },
        { key: 'systemPrompt', label: t('chat.contextPanel.systemPrompt'), tokens: breakdown.systemPromptTokens, dot: 'bg-clay/60' },
        { key: 'systemTools', label: t('chat.contextPanel.systemTools'), tokens: breakdown.systemToolsTokens, dot: 'bg-clay/35' },
      ]
    : []

  return (
    <div
      role="dialog"
      aria-label={t('chat.contextPanel.title')}
      data-context-usage-panel="true"
      className="absolute bottom-[calc(100%+10px)] right-0 z-50 w-[min(320px,86vw)] rounded-xl border border-line bg-paper px-3.5 py-3 shadow-[0_14px_36px_rgba(20,20,19,0.16)]"
    >
      <button
        type="button"
        className="flex w-full items-center gap-2 rounded-lg px-1 py-0.5 outline-none transition hover:bg-paper-hover focus-visible:ring-2 focus-visible:ring-clay/35"
        aria-expanded={expanded}
        aria-label={expanded ? t('chat.contextPanel.collapse') : t('chat.contextPanel.expand')}
        onClick={() => setExpanded((value) => !value)}
      >
        <span className="text-xs font-medium text-ink-soft">{t('chat.contextPanel.title')}</span>
        {usage ? (
          <span className="ml-auto font-mono text-xs tabular-nums text-ink">
            {formatTokenCount(usage.usedTokens)} / {formatTokenCount(usage.totalTokens)} ({usedPercent}%)
          </span>
        ) : (
          <span className="ml-auto text-xs text-ink-faint">{t('chat.contextUsageUnavailable')}</span>
        )}
        <ChevronRight
          size={14}
          aria-hidden="true"
          className={`shrink-0 text-ink-faint transition-transform ${expanded ? 'rotate-90' : ''}`}
        />
      </button>

      <div className="mt-2 h-1 overflow-hidden rounded-full bg-paper-hover">
        <div
          className={`h-full rounded-full transition-[width] duration-300 ${barTone}`}
          style={{ width: `${usedPercent}%` }}
        />
      </div>

      {expanded ? (
        <div className="mt-2.5" data-context-usage-breakdown="true">
          {categories.length > 0 ? (
            <ul className="grid gap-1">
              {categories.map((category) => (
                <li key={category.key} className="flex items-center gap-2 px-1 text-xs">
                  <span aria-hidden="true" className={`size-2 shrink-0 rounded-[3px] ${category.dot}`} />
                  <span className="text-ink-soft">{category.label}</span>
                  <span className="ml-auto font-mono tabular-nums text-ink">
                    {formatTokenCount(category.tokens)}
                  </span>
                  <span className="w-12 text-right font-mono tabular-nums text-ink-faint">
                    {usage ? formatSharePercent(category.tokens, usage.totalTokens) : '—'}
                  </span>
                </li>
              ))}
            </ul>
          ) : (
            <p className="px-1 text-[11px] text-ink-faint">{t('chat.contextPanel.breakdownUnavailable')}</p>
          )}
          {usage?.estimated ? (
            <p className="mt-2 px-1 text-[11px] text-ink-faint">{t('chat.contextUsageEstimated')}</p>
          ) : null}
          {onShowDetails ? (
            <button
              type="button"
              className="mt-2.5 w-full rounded-lg border border-line bg-surface/60 px-3 py-1.5 text-xs font-medium text-ink-soft outline-none transition hover:bg-paper-hover hover:text-ink focus-visible:ring-2 focus-visible:ring-clay/35"
              onClick={onShowDetails}
            >
              {t('chat.contextPanel.details')}
            </button>
          ) : null}
        </div>
      ) : null}
    </div>
  )
}

function usagePercent(usage: ContextUsage): number {
  return Math.min(100, Math.max(0, Math.round((usage.usedTokens / usage.totalTokens) * 100)))
}

function formatSharePercent(tokens: number, totalTokens: number): string {
  if (totalTokens <= 0) return '—'
  const share = Math.max(0, (tokens / totalTokens) * 100)
  return `${share >= 10 ? Math.round(share) : Math.round(share * 10) / 10}%`
}

export function formatTokenCount(tokens: number): string {
  if (tokens < 1_000) return String(tokens)
  if (tokens < 1_000_000) return `${formatUnit(tokens / 1_000)}k`
  return `${formatUnit(tokens / 1_000_000)}m`
}

function formatUnit(value: number): string {
  return value >= 10 ? String(Math.round(value)) : String(Math.round(value * 10) / 10)
}
