import { useCallback, useEffect, useMemo, useState } from 'react'
import { RefreshCw, Search } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { coreCommands } from '@/bridge/commands'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import type { RuntimeTraceSummary } from '@/bridge/compat'
import { resolveErrorMessage } from '@/lib/commandError'
import { useSessionStore } from '@/features/sessions/sessionStore'
import { useNavigationStore } from '@/app/navigationStore'
import { useProjectStore } from '@/stores/projectStore'
import {
  buildTraceListItems,
  filterTraceListItems,
  shouldPollTrace,
  type TraceStatusFilter,
} from '../traceViewModel'
import { TraceList } from './TraceList'
import { TurnTraceDrawer } from './TurnTraceDrawer'

const STATUS_OPTIONS: TraceStatusFilter[] = ['all', 'running', 'completed', 'failed', 'cancelled', 'interrupted']

export function TracePage() {
  const { t } = useTranslation()
  const [summaries, setSummaries] = useState<RuntimeTraceSummary[]>([])
  const [query, setQuery] = useState('')
  const [status, setStatus] = useState<TraceStatusFilter>('all')
  const [selectedTraceId, setSelectedTraceId] = useState<string | null>(null)
  const [limit, setLimit] = useState(100)
  const [now, setNow] = useState(() => Date.now())
  const [isLoading, setIsLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const sessions = useSessionStore((state) => state.summaries)
  const selectSession = useSessionStore((state) => state.select)
  const navigate = useNavigationStore((state) => state.navigate)
  const requestMessageFocus = useNavigationStore((state) => state.requestMessageFocus)
  const openDirectory = useProjectStore((state) => state.openDirectory)

  const load = useCallback(async (nextLimit = limit, silently = false) => {
    if (!silently) setIsLoading(true)
    setError(null)
    try {
      setSummaries(await coreCommands.listTraces(undefined, nextLimit))
    } catch (reason) {
      setError(resolveErrorMessage(reason))
    } finally {
      if (!silently) setIsLoading(false)
    }
  }, [limit])

  useEffect(() => { void load(limit) }, [limit, load])

  const hasRunningTrace = useMemo(
    () => summaries.some((summary) => shouldPollTrace(summary.status, [])),
    [summaries],
  )
  useEffect(() => {
    if (!hasRunningTrace) return
    const durationTimer = window.setInterval(() => setNow(Date.now()), 1_000)
    const refreshTimer = window.setInterval(() => void load(limit, true), 3_000)
    return () => {
      window.clearInterval(durationTimer)
      window.clearInterval(refreshTimer)
    }
  }, [hasRunningTrace, limit, load])

  const context = useMemo(
    () => Object.fromEntries(Object.values(sessions).map((session) => [session.id, {
      title: session.title,
      workingDirectory: session.workingDirectory,
    }])),
    [sessions],
  )
  const items = useMemo(() => buildTraceListItems(summaries, context, now), [context, now, summaries])
  const visible = useMemo(() => filterTraceListItems(items, query, status), [items, query, status])
  const selected = useMemo(
    () => items.find((item) => item.traceId === selectedTraceId) ?? null,
    [items, selectedTraceId],
  )

  const openMessage = useCallback(async (sessionId: string, messageId: string) => {
    const session = sessions[sessionId]
    if (session) openDirectory(session.workingDirectory)
    await selectSession(sessionId)
    requestMessageFocus(sessionId, messageId)
    navigate('chat')
  }, [navigate, openDirectory, requestMessageFocus, selectSession, sessions])

  return (
    <div className="relative h-full overflow-auto bg-paper">
      <div className="mx-auto w-full max-w-6xl p-8 max-[640px]:p-5">
        <div className="flex items-start justify-between gap-4">
          <p className="max-w-2xl text-sm leading-6 text-ink-faint">{t('activity.description')}</p>
          <Button type="button" variant="ghost" size="sm" disabled={isLoading} onClick={() => void load()}>
            <RefreshCw size={15} className={isLoading ? 'animate-spin' : ''} />
            {t('activity.refresh')}
          </Button>
        </div>
        <div className="mt-6 flex gap-3 max-[640px]:flex-col">
          <label className="flex h-10 min-w-0 flex-1 items-center gap-2 rounded-xl border border-line bg-surface px-3 focus-within:border-clay">
            <Search size={16} className="text-ink-faint" />
            <input
              value={query}
              aria-label={t('activity.search')}
              onChange={(event) => setQuery(event.target.value)}
              placeholder={t('activity.searchPlaceholder')}
              className="min-w-0 flex-1 bg-transparent text-sm outline-none"
            />
          </label>
          <Select value={status} onValueChange={(value) => setStatus(value as TraceStatusFilter)}>
            <SelectTrigger className="h-10 min-w-40 border border-line px-3" aria-label={t('activity.statusLabel')}><SelectValue /></SelectTrigger>
            <SelectContent>
              {STATUS_OPTIONS.map((value) => (
                <SelectItem key={value} value={value}>{t(`activity.status.${value}`)}</SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        {error ? <p role="alert" className="mt-5 rounded-xl bg-status-danger-soft p-3 text-sm text-status-danger-ink">{error}</p> : null}
        <div className="mt-5">
          <TraceList
            items={visible}
            loading={isLoading}
            onOpen={(item) => setSelectedTraceId(item.traceId)}
          />
        </div>
        {summaries.length === limit && limit < 500 ? (
          <div className="mt-5 text-center">
            <Button type="button" onClick={() => setLimit((value) => Math.min(500, value + 100))}>{t('activity.loadMore')}</Button>
          </div>
        ) : null}
      </div>
      {selected ? (
        <TurnTraceDrawer
          source={selected.turnId
            ? { kind: 'turn', turnId: selected.turnId }
            : { kind: 'trace', traceId: selected.traceId }}
          summary={selected}
          onOpenMessage={(sessionId, messageId) => void openMessage(sessionId, messageId)}
          onClose={() => setSelectedTraceId(null)}
        />
      ) : null}
    </div>
  )
}

export default TracePage
