import { useEffect, useMemo, useState } from 'react'
import { Activity, AlertCircle, Clock3, RefreshCw, Search } from 'lucide-react'
import { AnimatePresence } from 'motion/react'
import { useTranslation } from 'react-i18next'

import { sessionsApi } from '../../api/sessions'
import { useSessionStore } from '../../stores/sessionStore'
import type { SessionSummary } from '../../type/session'
import type { TraceSpanStatus, TurnTraceSummary } from '../../type/trace'
import { resolveErrorMessage } from '../../utils/commandError'
import { TurnTracePanel } from '../trace/TurnTracePanel'
import { formatDuration } from '../trace/traceViewModel'
import { Button } from '../ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../ui/select'

const PAGE_SIZE = 50

export type TraceStatusFilter = 'all' | TraceSpanStatus

export function filterTraceList(
  summaries: TurnTraceSummary[],
  sessions: SessionSummary[],
  query: string,
  status: TraceStatusFilter,
): TurnTraceSummary[] {
  const normalizedQuery = query.trim().toLocaleLowerCase()
  const sessionsById = new Map(sessions.map((session) => [session.id, session]))
  return summaries.filter((summary) => {
    if (status !== 'all' && summary.status !== status) return false
    if (!normalizedQuery) return true
    const session = sessionsById.get(summary.sessionId)
    const searchable = [
      summary.turnId,
      summary.model,
      session?.title,
      session?.workingDir,
      projectName(session?.workingDir),
    ]
      .filter(Boolean)
      .join(' ')
      .toLocaleLowerCase()
    return searchable.includes(normalizedQuery)
  })
}

export function TraceSettings() {
  const { t } = useTranslation()
  const sessions = useSessionStore((state) => state.sessions)
  const [summaries, setSummaries] = useState<TurnTraceSummary[]>([])
  const [query, setQuery] = useState('')
  const [status, setStatus] = useState<TraceStatusFilter>('all')
  const [nextOffset, setNextOffset] = useState<number | null>(null)
  const [selectedTurnId, setSelectedTurnId] = useState<string | null>(null)
  const [isLoading, setIsLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  async function load(offset: number, append: boolean) {
    setIsLoading(true)
    setError(null)
    try {
      const page = await sessionsApi.traceList(PAGE_SIZE, offset)
      setSummaries((current) => (append ? [...current, ...page.items] : page.items))
      setNextOffset(page.nextOffset)
    } catch (reason) {
      setError(resolveErrorMessage(reason))
    } finally {
      setIsLoading(false)
    }
  }

  useEffect(() => {
    void load(0, false)
  }, [])

  return (
    <div className="relative h-full bg-paper">
      <div className="h-full overflow-auto">
        <div className="mx-auto w-full max-w-5xl p-8 max-[640px]:p-5">
          <div className="flex items-start justify-between gap-4">
            <div>
              <h2 className="font-sans text-2xl font-semibold text-ink">{t('settings.trace.title')}</h2>
              <p className="mt-2 font-sans text-sm text-ink-faint">{t('settings.trace.description')}</p>
            </div>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              disabled={isLoading}
              onClick={() => void load(0, false)}
            >
              <RefreshCw size={15} className={isLoading ? 'animate-spin' : ''} />
              {t('settings.trace.refresh')}
            </Button>
          </div>

          <div className="mt-7 flex gap-3 max-[640px]:flex-col">
            <label className="flex h-10 min-w-0 flex-1 items-center gap-2 rounded-xl border border-line bg-paper px-3 focus-within:border-clay">
              <Search size={16} className="shrink-0 text-ink-faint" />
              <span className="sr-only">{t('settings.trace.search')}</span>
              <input
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                placeholder={t('settings.trace.searchPlaceholder')}
                className="min-w-0 flex-1 bg-transparent font-sans text-sm text-ink outline-none placeholder:text-ink-faint"
              />
            </label>
            <Select value={status} onValueChange={(value) => setStatus(value as TraceStatusFilter)}>
              <SelectTrigger className="h-10 min-w-40 border border-line px-3">
                <SelectValue aria-label={t('settings.trace.statusLabel')} />
              </SelectTrigger>
              <SelectContent>
                {statusOptions.map((value) => (
                  <SelectItem key={value} value={value}>
                    {t(`settings.trace.status.${value}`)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          {error ? <p className="mt-5 rounded-xl bg-clay-soft p-3 font-sans text-sm text-ink">{error}</p> : null}
          <div className="mt-5">
            <TraceSettingsList
              summaries={summaries}
              sessions={sessions}
              query={query}
              status={status}
              onOpen={setSelectedTurnId}
            />
          </div>
          {isLoading && summaries.length === 0 ? (
            <p className="py-10 text-center font-sans text-sm text-ink-faint">{t('settings.trace.loading')}</p>
          ) : null}
          {nextOffset !== null ? (
            <div className="mt-5 flex justify-center">
              <Button
                type="button"
                variant="ghost"
                disabled={isLoading}
                onClick={() => void load(nextOffset, true)}
              >
                {isLoading ? t('settings.trace.loading') : t('settings.trace.loadMore')}
              </Button>
            </div>
          ) : null}
        </div>
      </div>
      <AnimatePresence>
        {selectedTurnId ? (
          <TurnTracePanel turnId={selectedTurnId} onClose={() => setSelectedTurnId(null)} />
        ) : null}
      </AnimatePresence>
    </div>
  )
}

export function TraceSettingsList({
  summaries,
  sessions,
  query,
  status,
  onOpen,
}: {
  summaries: TurnTraceSummary[]
  sessions: SessionSummary[]
  query: string
  status: TraceStatusFilter
  onOpen: (turnId: string) => void
}) {
  const { i18n, t } = useTranslation()
  const filtered = useMemo(
    () => filterTraceList(summaries, sessions, query, status),
    [query, sessions, status, summaries],
  )
  const sessionsById = useMemo(
    () => new Map(sessions.map((session) => [session.id, session])),
    [sessions],
  )

  if (filtered.length === 0) {
    return (
      <div data-settings-trace-list="true" className="rounded-2xl border border-dashed border-line py-12 text-center">
        <Activity size={22} className="mx-auto text-ink-faint" />
        <p className="mt-3 font-sans text-sm text-ink-faint">{t('settings.trace.empty')}</p>
      </div>
    )
  }

  return (
    <div data-settings-trace-list="true" className="overflow-hidden rounded-2xl border border-line bg-surface">
      {filtered.map((summary) => {
        const session = sessionsById.get(summary.sessionId)
        const sessionTitle = session?.title ?? t('settings.trace.unknownSession')
        const project = projectName(session?.workingDir) ?? t('settings.trace.unknownProject')
        return (
          <button
            key={summary.turnId}
            type="button"
            data-trace-row={summary.turnId}
            aria-label={`${t('settings.trace.openDetail')}: ${sessionTitle}`}
            className="group flex w-full items-center gap-4 border-b border-line px-4 py-3 text-left last:border-b-0 hover:bg-paper-hover"
            onClick={() => onOpen(summary.turnId)}
          >
            <span className={`size-2.5 shrink-0 rounded-full ${statusColor(summary.status)}`} />
            <span className="min-w-0 flex-1">
              <span className="flex min-w-0 items-center gap-2">
                <span className="truncate font-sans text-sm font-medium text-ink">{sessionTitle}</span>
                <span className="shrink-0 rounded-md bg-paper-hover px-1.5 py-0.5 font-sans text-[11px] text-ink-faint">
                  {t(`settings.trace.status.${summary.status}`)}
                </span>
              </span>
              <span className="mt-1 flex flex-wrap gap-x-3 gap-y-1 font-sans text-xs text-ink-faint">
                <span>{project}</span>
                <span>{summary.model ?? t('trace.unknownModel')}</span>
                <span>{t('settings.trace.steps', { count: summary.stepCount })}</span>
                <span>{t('settings.trace.tools', { count: summary.toolRunCount })}</span>
                {summary.inputTokens + summary.outputTokens > 0 ? (
                  <span>{t('settings.trace.tokens', { count: summary.inputTokens + summary.outputTokens })}</span>
                ) : null}
                {summary.retryCount > 0 ? <span>{t('settings.trace.retryCount', { count: summary.retryCount })}</span> : null}
                {summary.errorCount > 0 ? <span className="text-clay">{t('settings.trace.errorCount', { count: summary.errorCount })}</span> : null}
              </span>
            </span>
            <span className="hidden shrink-0 text-right font-sans text-xs text-ink-faint sm:block">
              <span className="block">{formatStartedAt(summary.startedAt, i18n.language)}</span>
              <span className="mt-1 flex items-center justify-end gap-1">
                {summary.errorCount > 0 ? <AlertCircle size={12} className="text-clay" /> : <Clock3 size={12} />}
                {formatDuration(summary.durationMs, i18n.language)}
              </span>
            </span>
          </button>
        )
      })}
    </div>
  )
}

const statusOptions: TraceStatusFilter[] = [
  'all',
  'running',
  'waiting',
  'succeeded',
  'failed',
  'cancelled',
  'denied',
  'outcome_unknown',
]

function projectName(path: string | null | undefined): string | null {
  if (!path) return null
  const normalized = path.replace(/[\\/]+$/, '')
  return normalized.split(/[\\/]/).pop() || normalized
}

function formatStartedAt(timestamp: number, language: string): string {
  return new Intl.DateTimeFormat(language, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  }).format(timestamp)
}

function statusColor(status: TraceSpanStatus): string {
  if (status === 'failed' || status === 'denied') return 'bg-clay'
  if (status === 'running' || status === 'waiting') return 'bg-amber-500'
  if (status === 'succeeded') return 'bg-emerald-500'
  return 'bg-ink-faint'
}
