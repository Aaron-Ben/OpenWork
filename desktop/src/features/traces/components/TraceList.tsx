import { Activity, Bot, ChevronRight, Minimize2, Wrench } from 'lucide-react'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import {
  traceDayKey,
  type TraceListItem,
} from '../traceViewModel'

const DISPLAY_TIME_ZONE = 'Asia/Shanghai'

interface TraceListProps {
  items: TraceListItem[]
  loading: boolean
  onOpen: (item: TraceListItem) => void
  now?: number
}

interface TraceDateGroup {
  key: string
  items: TraceListItem[]
}

export function TraceList({ items, loading, onOpen, now = Date.now() }: TraceListProps) {
  const { t, i18n } = useTranslation()
  const groups = useMemo(() => groupTraceItems(items), [items])
  const maxDurationMs = Math.max(0, ...items.map((item) => item.durationMs ?? 0))
  const maxTokens = Math.max(0, ...items.map((item) => item.totalTokens))

  if (loading) {
    return (
      <div data-trace-loading="true" className="grid gap-3" aria-label={t('activity.loading')}>
        {[0, 1, 2].map((index) => (
          <div key={index} className="h-24 animate-pulse rounded-2xl border border-line bg-surface" />
        ))}
      </div>
    )
  }
  if (items.length === 0) {
    return (
      <div className="rounded-2xl border border-dashed border-line py-12 text-center">
        <Activity size={22} className="mx-auto text-ink-faint" />
        <p className="mt-2.5 text-sm text-ink-faint">{t('activity.empty')}</p>
      </div>
    )
  }

  return (
    <div className="grid gap-7" aria-label={t('activity.title')}>
      {groups.map((group) => (
        <section key={group.key} data-trace-date-group={group.key}>
          <div className="mb-3 flex items-center gap-3 px-1">
            <h2 className="shrink-0 text-xs font-semibold text-ink-soft">
              {formatDateGroupLabel(group.key, now, i18n.language, t)}
            </h2>
            <span className="h-px flex-1 bg-line" aria-hidden="true" />
          </div>
          <div className="grid gap-2.5">
            {group.items.map((item) => (
              <TraceRunCard
                key={item.traceId}
                item={item}
                maxDurationMs={maxDurationMs}
                maxTokens={maxTokens}
                now={now}
                language={i18n.language}
                onOpen={onOpen}
              />
            ))}
          </div>
        </section>
      ))}
    </div>
  )
}

function TraceRunCard({
  item,
  maxDurationMs,
  maxTokens,
  now,
  language,
  onOpen,
}: {
  item: TraceListItem
  maxDurationMs: number
  maxTokens: number
  now: number
  language: string
  onOpen: (item: TraceListItem) => void
}) {
  const { t } = useTranslation()
  // 没有 Turn 的 Trace 是一次独立压缩：不显示并不存在的调用计数，
  // 但 token 是 span 实测合计，照常显示；卡片仍可通过 trace_id 打开同一个详情抽屉。
  const isTurnless = item.turnId == null
  const durationPercent = scalePercent(item.durationMs ?? 0, maxDurationMs)
  const tokenPercent = scalePercent(item.totalTokens, maxTokens)

  return (
    <button
      type="button"
      data-trace-row={item.traceId}
      data-trace-run-card={item.traceId}
      data-model-calls={isTurnless ? undefined : item.modelSubmissionCount}
      data-tool-calls={isTurnless ? undefined : item.toolCallCount}
      data-total-tokens={item.totalTokens}
      data-duration-percent={durationPercent}
      data-token-percent={tokenPercent}
      onClick={() => onOpen(item)}
      className={`relative grid w-full grid-cols-[minmax(0,1fr)_minmax(190px,0.42fr)_130px_18px] items-center gap-6 overflow-hidden rounded-2xl border px-8 py-3 text-left transition-colors hover:bg-paper-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-clay/35 max-[820px]:grid-cols-[minmax(0,1fr)_96px] max-[820px]:gap-3 max-[820px]:px-6 ${runCardClass(item.status)}`}
    >
      <span className={`absolute inset-y-0 left-0 w-2 ${runAccentClass(item.status)}`} aria-hidden="true" />

      <span className="min-w-0">
        <span className="flex items-center gap-2">
          <span className="truncate text-sm font-semibold text-ink">{item.title}</span>
          <StatusBadge status={item.status} />
          {isTurnless ? (
            <span className="inline-flex shrink-0 items-center gap-1 rounded-full bg-paper-hover px-2 py-0.5 text-[10px] text-ink-soft">
              <Minimize2 size={10} aria-hidden="true" />
              {t('activity.compaction')}
            </span>
          ) : null}
        </span>
        <span className="mt-1 block truncate font-mono text-[11px] text-ink-faint" title={item.workingDirectory || item.sessionId}>
          {item.workingDirectory || item.sessionId}
        </span>
        <span className="mt-1.5 flex min-w-0 items-center gap-2 font-mono text-[10px] text-ink-soft">
          {!isTurnless ? (
            <>
              <span className="inline-flex shrink-0 items-center gap-1">
                <Bot size={11} className="text-clay" aria-hidden="true" />
                {t('activity.runCard.modelCount', { count: item.modelSubmissionCount })}
              </span>
              <span aria-hidden="true">·</span>
              <span className="inline-flex shrink-0 items-center gap-1">
                <Wrench size={11} className="text-status-success" aria-hidden="true" />
                {t('activity.runCard.toolCount', { count: item.toolCallCount })}
              </span>
              <span aria-hidden="true">·</span>
            </>
          ) : null}
          <span className="truncate">{item.resolvedModelName || '—'}</span>
        </span>
      </span>

      <span className="grid gap-2 max-[820px]:col-span-1 max-[820px]:row-start-2">
        <MetricBar
          label={t('activity.runCard.duration')}
          value={formatDuration(item.durationMs)}
          percent={durationPercent}
          color="bg-clay"
        />
        <MetricBar
          label={t('activity.runCard.token')}
          value={item.totalTokens.toLocaleString(language)}
          percent={tokenPercent}
          color="bg-status-success"
        />
      </span>

      <span className="text-right max-[820px]:col-start-2 max-[820px]:row-span-2 max-[820px]:row-start-1">
        <span className="block text-xs font-semibold text-ink">
          {formatRelativeTime(item.startedAt, now, t)}
        </span>
        <span className="mt-0.5 block font-mono text-[10px] tabular-nums text-ink-faint">
          {formatClockTime(item.startedAt, language)}
        </span>
      </span>

      <ChevronRight size={16} className="text-ink-faint max-[820px]:hidden" aria-hidden="true" />
    </button>
  )
}

function MetricBar({
  label,
  value,
  percent,
  color,
}: {
  label: string
  value: string
  percent: number
  color: string
}) {
  return (
    <span className="block">
      <span className="flex items-center justify-between gap-3 text-[10px] text-ink-faint">
        <span>{label}</span>
        <span className="font-mono text-xs font-semibold tabular-nums text-ink">{value}</span>
      </span>
      <span className="mt-1 block h-1 overflow-hidden rounded-full bg-line/70">
        <span className={`block h-full rounded-full ${color}`} style={{ width: `${percent}%` }} />
      </span>
    </span>
  )
}

function groupTraceItems(items: TraceListItem[]): TraceDateGroup[] {
  return items.reduce<TraceDateGroup[]>((groups, item) => {
    const key = traceDayKey(item.startedAt) ?? item.startedAt
    const current = groups[groups.length - 1]
    if (current?.key === key) {
      current.items.push(item)
      return groups
    }
    groups.push({ key, items: [item] })
    return groups
  }, [])
}

function formatDateGroupLabel(
  key: string,
  now: number,
  language: string,
  t: ReturnType<typeof useTranslation>['t'],
): string {
  const todayKey = traceDayKey(now)
  const yesterdayKey = traceDayKey(now - 24 * 60 * 60 * 1000)
  const relative = key === todayKey
    ? t('activity.runCard.today')
    : key === yesterdayKey
      ? t('activity.runCard.yesterday')
      : null
  const date = new Intl.DateTimeFormat(language, {
    month: 'long',
    day: 'numeric',
    timeZone: DISPLAY_TIME_ZONE,
  }).format(new Date(`${key}T00:00:00+08:00`))
  return relative ? `${relative} · ${date}` : date
}

function formatRelativeTime(
  startedAt: string,
  now: number,
  t: ReturnType<typeof useTranslation>['t'],
): string {
  const elapsedMs = Math.max(0, now - Date.parse(startedAt))
  const minutes = Math.floor(elapsedMs / 60_000)
  if (minutes < 1) return t('activity.runCard.justNow')
  if (minutes < 60) return t('activity.runCard.minutesAgo', { count: minutes })
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return t('activity.runCard.hoursAgo', { count: hours })
  return t('activity.runCard.daysAgo', { count: Math.floor(hours / 24) })
}

function formatClockTime(startedAt: string, language: string): string {
  return new Intl.DateTimeFormat(language, {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    hourCycle: 'h23',
    timeZone: DISPLAY_TIME_ZONE,
  }).format(new Date(startedAt))
}

function scalePercent(value: number, max: number): number {
  if (value <= 0 || max <= 0) return 0
  return Math.round((value / max) * 100)
}

function StatusBadge({ status }: { status: string }) {
  const { t } = useTranslation()
  return (
    <span className={`inline-flex shrink-0 items-center rounded-full px-2 py-0.5 text-[10px] font-semibold ${statusBadge(status)}`}>
      {t(`activity.status.${status}` as 'activity.status.completed', { defaultValue: status })}
    </span>
  )
}

function runCardClass(status: string): string {
  if (status === 'failed') return 'border-status-danger-border/45 bg-status-danger-soft/30'
  if (status === 'running') return 'border-clay/30 bg-clay-soft/25'
  return 'border-line bg-paper'
}

function runAccentClass(status: string): string {
  if (status === 'completed') return 'bg-status-success'
  if (status === 'running') return 'bg-clay'
  if (status === 'cancelled' || status === 'interrupted') return 'bg-ink-faint'
  return 'bg-status-danger'
}

function statusBadge(status: string): string {
  if (status === 'completed') return 'bg-status-success-soft text-status-success'
  if (status === 'running') return 'bg-status-warning-soft text-status-warning-ink'
  if (status === 'cancelled' || status === 'interrupted') return 'bg-paper-hover text-ink-soft'
  return 'bg-status-danger-soft text-status-danger-ink'
}

export function formatDuration(durationMs: number | null): string {
  if (durationMs == null) return '—'
  if (durationMs < 1000) return `${Math.round(durationMs)} ms`
  return `${(durationMs / 1000).toFixed(2)} s`
}
