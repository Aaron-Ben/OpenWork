import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabLogEntry } from '@/bridge/collab'
import { Button } from '@/components/ui/button'
import { formatBeijingDateTime } from '@/lib/dateTime'
import { useLogStore } from './logStore'

export function LogDrawer({ activeRoomId }: { activeRoomId: string | null }) {
  const { t } = useTranslation()
  const entries = useLogStore((state) => state.entries)
  const roomId = useLogStore((state) => state.roomId)
  const loading = useLogStore((state) => state.loading)
  const error = useLogStore((state) => state.error)
  const fetch = useLogStore((state) => state.fetch)

  useEffect(() => {
    void fetch(null)
  }, [fetch])

  return (
    <section className="flex min-w-0 flex-1 flex-col" aria-label={t('collab.logs.title')}>
      <header data-tauri-drag-region="deep" className="flex h-14 shrink-0 items-center gap-2 border-b border-line px-5">
        <div className="mr-auto">
          <h1 className="text-sm font-semibold">{t('collab.logs.title')}</h1>
          <p className="text-xs text-ink-faint">{t('collab.logs.description')}</p>
        </div>
        <Button size="sm" variant={roomId === null ? 'outline' : 'ghost'} onClick={() => void fetch(null)}>
          {t('collab.logs.allRooms')}
        </Button>
        {activeRoomId ? (
          <Button size="sm" variant={roomId === activeRoomId ? 'outline' : 'ghost'} onClick={() => void fetch(activeRoomId)}>
            {t('collab.logs.currentRoom')}
          </Button>
        ) : null}
        <Button size="sm" variant="ghost" onClick={() => void fetch(roomId)}>{t('collab.logs.refresh')}</Button>
      </header>
      <div className="min-h-0 flex-1 overflow-y-auto p-5">
        {error ? <p className="mb-3 text-sm text-red-600">{error}</p> : null}
        {loading && entries.length === 0 ? <p className="text-sm text-ink-faint">{t('collab.logs.loading')}</p> : null}
        {!loading && entries.length === 0 ? <p className="text-sm text-ink-faint">{t('collab.logs.empty')}</p> : null}
        <LogTimeline entries={entries} />
      </div>
    </section>
  )
}

export function LogTimeline({ entries }: { entries: readonly CollabLogEntry[] }) {
  const { t } = useTranslation()
  return (
    <ol className="space-y-2">
      {entries.map((entry) => (
        <li
          key={`${entry.source}:${entry.id}`}
          data-log-source={entry.source}
          data-run-outcome={typeof entry.payload.outcome === 'string' ? entry.payload.outcome : undefined}
          className={`rounded-xl border bg-paper px-4 py-3 ${entry.payload.outcome === 'unpublished' ? 'border-red-500 bg-red-50' : 'border-line'}`}
        >
          <div className="flex flex-wrap items-center gap-2 text-xs">
            <span className="rounded-full bg-paper-hover px-2 py-0.5 font-medium text-ink-muted">
              {t(`collab.logs.sources.${entry.source}`)}
            </span>
            <strong className="font-mono text-ink">{entry.kind}</strong>
            {typeof entry.payload.outcome === 'string' ? (
              <span className={`rounded-full px-2 py-0.5 font-semibold ${entry.payload.outcome === 'unpublished' ? 'bg-red-600 text-white' : 'bg-paper-hover text-ink-muted'}`}>
                {t(`collab.logs.outcomes.${entry.payload.outcome}`)}
              </span>
            ) : null}
            {entry.agentId ? <span>{entry.agentId}</span> : null}
            {entry.roomId ? <span>#{entry.roomId}</span> : null}
            {entry.runId ? <span className="font-mono text-ink-faint">{entry.runId}</span> : null}
            <time className="ml-auto text-ink-faint" dateTime={entry.createdAt}>
              {formatBeijingDateTime(entry.createdAt)}
            </time>
          </div>
          <p className="mt-2 text-sm text-ink-muted">
            {logSummary(entry.payload, {
              actionable: t('collab.logs.actionable'),
              notActionable: t('collab.logs.notActionable'),
              tokenSummary: (input, cached, output) => t('collab.logs.tokenSummary', {
                input,
                cached,
                output,
              }),
            })}
          </p>
          <details className="mt-2 text-xs text-ink-faint">
            <summary className="cursor-pointer">{t('collab.logs.payload')}</summary>
            <pre className="mt-2 overflow-x-auto whitespace-pre-wrap break-words rounded-lg bg-paper-hover p-3">{JSON.stringify(entry.payload, null, 2)}</pre>
          </details>
        </li>
      ))}
    </ol>
  )
}

interface LogSummaryLabels {
  actionable: string
  notActionable: string
  tokenSummary: (input: number, cached: number, output: number) => string
}

export function logSummary(
  payload: Readonly<Record<string, unknown>>,
  labels: LogSummaryLabels,
): string {
  for (const key of ['reason', 'errorMessage', 'error', 'tool', 'command', 'outcome', 'trigger', 'status']) {
    const value = payload[key]
    if (typeof value === 'string' && value.length > 0) return value
  }
  if (typeof payload.actionable === 'boolean') {
    return payload.actionable ? labels.actionable : labels.notActionable
  }
  const tokenParts = [payload.inputTokens, payload.cachedInputTokens, payload.outputTokens]
  if (tokenParts.some((value) => typeof value === 'number')) {
    return labels.tokenSummary(
      typeof tokenParts[0] === 'number' ? tokenParts[0] : 0,
      typeof tokenParts[1] === 'number' ? tokenParts[1] : 0,
      typeof tokenParts[2] === 'number' ? tokenParts[2] : 0,
    )
  }
  return '—'
}
