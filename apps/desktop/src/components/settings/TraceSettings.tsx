import { useEffect, useMemo, useState } from 'react'
import { Activity, Clock3, RefreshCw, Search } from 'lucide-react'
import { AnimatePresence } from 'motion/react'
import { useTranslation } from 'react-i18next'

import { runtimeApi } from '../../api/runtime'
import type { RuntimeTraceSummary } from '../../type/runtime'
import { resolveErrorMessage } from '../../utils/commandError'
import { RuntimeTracePanel } from '../trace/RuntimeTracePanel'
import { Button } from '../ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../ui/select'

export type TraceStatusFilter = 'all' | 'running' | 'completed' | 'failed' | 'cancelled' | 'interrupted'

export function filterRuntimeTraces(
  summaries: RuntimeTraceSummary[],
  query: string,
  status: TraceStatusFilter,
): RuntimeTraceSummary[] {
  const normalized = query.trim().toLowerCase()
  return summaries.filter((summary) => {
    if (status !== 'all' && summary.status !== status) return false
    if (!normalized) return true
    return [summary.turnId, summary.sessionId, summary.resolvedModelName]
      .some((value) => value.toLowerCase().includes(normalized))
  })
}

export function TraceSettings() {
  const { t } = useTranslation()
  const [summaries, setSummaries] = useState<RuntimeTraceSummary[]>([])
  const [query, setQuery] = useState('')
  const [status, setStatus] = useState<TraceStatusFilter>('all')
  const [selectedTurnId, setSelectedTurnId] = useState<string | null>(null)
  const [isLoading, setIsLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  async function load() {
    setIsLoading(true)
    setError(null)
    try {
      setSummaries(await runtimeApi.listTraces(undefined, 500))
    } catch (reason) {
      setError(resolveErrorMessage(reason))
    } finally {
      setIsLoading(false)
    }
  }

  useEffect(() => {
    void load()
  }, [])

  const visible = useMemo(
    () => filterRuntimeTraces(summaries, query, status),
    [query, status, summaries],
  )

  return (
    <div className="relative h-full bg-paper">
      <div className="h-full overflow-auto">
        <div className="mx-auto w-full max-w-5xl p-8 max-[640px]:p-5">
          <div className="flex items-start justify-between gap-4">
            <div>
              <h2 className="font-sans text-2xl font-semibold text-ink">{t('settings.trace.title')}</h2>
              <p className="mt-2 font-sans text-sm text-ink-faint">{t('settings.trace.description')}</p>
            </div>
            <Button type="button" variant="ghost" size="sm" disabled={isLoading} onClick={() => void load()}>
              <RefreshCw size={15} className={isLoading ? 'animate-spin' : ''} />
              {t('settings.trace.refresh')}
            </Button>
          </div>
          <div className="mt-7 flex gap-3 max-[640px]:flex-col">
            <label className="flex h-10 min-w-0 flex-1 items-center gap-2 rounded-xl border border-line bg-paper px-3 focus-within:border-clay">
              <Search size={16} className="text-ink-faint" />
              <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder={t('settings.trace.searchPlaceholder')} className="min-w-0 flex-1 bg-transparent text-sm outline-none" />
            </label>
            <Select value={status} onValueChange={(value) => setStatus(value as TraceStatusFilter)}>
              <SelectTrigger className="h-10 min-w-40 border border-line px-3"><SelectValue /></SelectTrigger>
              <SelectContent>
                {statusOptions.map((value) => <SelectItem key={value} value={value}>{value}</SelectItem>)}
              </SelectContent>
            </Select>
          </div>
          {error ? <p className="mt-5 rounded-xl bg-clay-soft p-3 text-sm text-ink">{error}</p> : null}
          <div className="mt-5">
            <TraceSettingsList summaries={visible} onOpen={setSelectedTurnId} />
          </div>
        </div>
      </div>
      <AnimatePresence>
        {selectedTurnId ? <RuntimeTracePanel turnId={selectedTurnId} onClose={() => setSelectedTurnId(null)} /> : null}
      </AnimatePresence>
    </div>
  )
}

export function TraceSettingsList({
  summaries,
  onOpen,
}: {
  summaries: RuntimeTraceSummary[]
  onOpen: (turnId: string) => void
}) {
  const { t } = useTranslation()
  if (summaries.length === 0) {
    return (
      <div data-settings-trace-list="true" className="rounded-2xl border border-dashed border-line py-12 text-center">
        <Activity size={22} className="mx-auto text-ink-faint" />
        <p className="mt-3 text-sm text-ink-faint">{t('settings.trace.empty')}</p>
      </div>
    )
  }
  return (
    <div data-settings-trace-list="true" className="overflow-hidden rounded-2xl border border-line bg-surface">
      {summaries.map((summary) => (
        <button key={summary.turnId} type="button" data-trace-row={summary.turnId} className="flex w-full items-center gap-4 border-b border-line px-4 py-3 text-left last:border-b-0 hover:bg-paper-hover" onClick={() => onOpen(summary.turnId)}>
          <span className={`size-2.5 rounded-full ${statusColor(summary.status)}`} />
          <span className="min-w-0 flex-1">
            <span className="block truncate text-sm font-medium text-ink">{summary.resolvedModelName}</span>
            <span className="mt-1 flex flex-wrap gap-3 text-xs text-ink-faint">
              <span>{summary.sessionId}</span>
              <span>models {summary.modelCallCount}</span>
              <span>tools {summary.toolCallCount}</span>
              <span>spans {summary.spanCount}</span>
            </span>
          </span>
          <span className="hidden text-right text-xs text-ink-faint sm:block">
            <span className="block">{formatStartedAt(summary.startedAt)}</span>
            <span className="mt-1 inline-flex items-center gap-1"><Clock3 size={12} />{duration(summary)} ms</span>
          </span>
        </button>
      ))}
    </div>
  )
}

const statusOptions: TraceStatusFilter[] = ['all', 'running', 'completed', 'failed', 'cancelled', 'interrupted']

function statusColor(status: string): string {
  if (status === 'completed') return 'bg-status-success'
  if (status === 'running') return 'bg-status-warning'
  return 'bg-status-danger'
}

function formatStartedAt(value: string): string {
  return new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' }).format(Date.parse(value))
}

function duration(summary: RuntimeTraceSummary): number {
  return summary.endedAt ? Math.max(0, Date.parse(summary.endedAt) - Date.parse(summary.startedAt)) : 0
}
