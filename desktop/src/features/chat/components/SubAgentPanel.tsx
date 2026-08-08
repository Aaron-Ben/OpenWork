import { useCallback, useEffect, useRef, useState } from 'react'
import { Bot, ChevronDown, ChevronRight, Clock3, LoaderCircle } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { coreCommands } from '@/bridge/commands'
import type {
  RuntimeLoadedSession,
  RuntimeStoredMessage,
  RuntimeSubAgentSessionRecord,
} from '@/bridge/compat'
import { resolveErrorMessage } from '@/lib/commandError'
import { formatDuration } from '@/features/traces/components/TraceList'
import { useRuntimeStore } from '../runtimeStore'
import type { SessionRuntimeView } from '../runtimeReducer'
import { AssistantMessage } from './AssistantMessage'
import { ToolActivityList } from './ToolActivityList'
import { UserMessage } from './UserMessage'

export type SubAgentDetailState =
  | { state: 'loading' }
  | { state: 'loaded'; session: RuntimeLoadedSession }
  | { state: 'error'; message: string }

export function SubAgentPanel({ parentSessionId }: { parentSessionId: string }) {
  const { t } = useTranslation()
  const [children, setChildren] = useState<RuntimeSubAgentSessionRecord[]>([])
  const [listError, setListError] = useState<string | null>(null)
  const [expandedId, setExpandedId] = useState<string | null>(null)
  const [details, setDetails] = useState<Record<string, SubAgentDetailState>>({})
  const refreshSequence = useRef(0)
  const runtimeBySession = useRuntimeStore((state) => state.bySession)
  const parentPhase = runtimeBySession[parentSessionId]?.phase ?? 'idle'

  const refresh = useCallback(async () => {
    const requestSequence = ++refreshSequence.current
    try {
      const listed = await coreCommands.listSubAgents(parentSessionId)
      if (requestSequence !== refreshSequence.current) return
      setChildren(listed)
      setListError(null)
      useRuntimeStore.getState().registerSubAgents(
        parentSessionId,
        listed.map((child) => child.id),
      )
    } catch (error) {
      if (requestSequence === refreshSequence.current) {
        setListError(resolveErrorMessage(error))
      }
    }
  }, [parentSessionId])

  useEffect(() => {
    let active = true
    void refresh()
    const timer = parentPhase === 'idle'
      ? null
      : window.setInterval(() => {
          if (active) void refresh()
        }, 1_000)
    return () => {
      active = false
      refreshSequence.current += 1
      if (timer !== null) window.clearInterval(timer)
    }
  }, [parentPhase, refresh])

  useEffect(() => {
    setChildren([])
    setListError(null)
    setExpandedId(null)
    setDetails({})
  }, [parentSessionId])

  const toggleDetails = useCallback((childId: string) => {
    if (expandedId === childId) {
      setExpandedId(null)
      return
    }
    setExpandedId(childId)
    if (details[childId]) return
    setDetails((current) => ({ ...current, [childId]: { state: 'loading' } }))
    void loadSubAgentTranscript(childId)
      .then((session) => {
        setDetails((current) => ({
          ...current,
          [childId]: { state: 'loaded', session },
        }))
      })
      .catch((error) => {
        setDetails((current) => ({
          ...current,
          [childId]: { state: 'error', message: resolveErrorMessage(error) },
        }))
      })
  }, [details, expandedId])

  if (children.length === 0 && !listError) return null

  return (
    <section className="mb-6 rounded-xl border border-line bg-surface" aria-label={t('chat.subAgents.title')}>
      <div className="flex items-center gap-2 border-b border-line px-4 py-3 text-sm font-medium text-ink">
        <Bot size={16} />
        {t('chat.subAgents.title')}
        {children.length > 0 ? (
          <span className="rounded-full bg-paper-hover px-2 py-0.5 text-xs text-ink-faint">
            {children.length}
          </span>
        ) : null}
      </div>
      {listError ? (
        <p className="px-4 py-3 text-xs text-status-danger-ink" role="alert">
          {t('chat.subAgents.listFailed', { reason: listError })}
        </p>
      ) : (
        <div className="divide-y divide-line">
          {children.map((child) => (
            <SubAgentPanelRow
              key={child.id}
              child={child}
              runtime={runtimeBySession[child.id]}
              expanded={expandedId === child.id}
              detail={details[child.id]}
              onToggle={toggleDetails}
            />
          ))}
        </div>
      )}
    </section>
  )
}

export function loadSubAgentTranscript(childSessionId: string): Promise<RuntimeLoadedSession> {
  return coreCommands.loadSession(childSessionId)
}

export function SubAgentPanelRow({
  child,
  runtime,
  expanded,
  detail,
  onToggle,
}: {
  child: RuntimeSubAgentSessionRecord
  runtime: SessionRuntimeView | undefined
  expanded: boolean
  detail: SubAgentDetailState | undefined
  onToggle: (childSessionId: string) => void
}) {
  const { t } = useTranslation()
  const presentation = subAgentPresentation(runtime, t)
  return (
    <div>
      <button
        type="button"
        data-sub-agent-id={child.id}
        aria-expanded={expanded}
        className="flex w-full items-start gap-3 px-4 py-3 text-left transition hover:bg-paper-hover"
        onClick={() => onToggle(child.id)}
      >
        {expanded ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
            <span className="font-mono text-sm font-medium text-ink">{child.taskName}</span>
            <span className="rounded-full border border-line px-2 py-0.5 text-[11px] text-ink-faint">
              {child.agentRole}
            </span>
            <span className={presentation.statusClass}>{presentation.statusLabel}</span>
            <span className="inline-flex items-center gap-1 text-xs text-ink-faint">
              <Clock3 size={12} />
              {formatDuration(presentation.durationMs)}
            </span>
          </div>
          {presentation.summary ? (
            <p className="mt-1.5 line-clamp-2 text-xs leading-5 text-ink-soft">
              {presentation.summary}
            </p>
          ) : null}
        </div>
      </button>
      {expanded ? (
        <div className="border-t border-line bg-paper px-4 py-4">
          {detail?.state === 'loaded' ? (
            <ReadonlySubAgentTranscript messages={detail.session.messages} />
          ) : detail?.state === 'error' ? (
            <p className="text-xs text-status-danger-ink" role="alert">
              {t('chat.subAgents.transcriptFailed', { reason: detail.message })}
            </p>
          ) : (
            <p className="inline-flex items-center gap-2 text-xs text-ink-faint" role="status">
              <LoaderCircle size={13} className="animate-spin" />
              {t('chat.subAgents.loadingTranscript')}
            </p>
          )}
        </div>
      ) : null}
    </div>
  )
}

function subAgentPresentation(
  runtime: SessionRuntimeView | undefined,
  t: ReturnType<typeof useTranslation>['t'],
) {
  const terminal = runtime?.terminal
  const running = runtime ? runtime.phase !== 'idle' : false
  const status = running ? 'running' : terminal?.status ?? 'idle'
  const statusLabel = t(`chat.subAgents.status.${status}`)
  const statusClass = status === 'completed'
    ? 'text-xs text-status-success-ink'
    : status === 'failed' || status === 'cancelled'
      ? 'text-xs text-status-danger-ink'
      : status === 'running'
        ? 'text-xs text-clay'
        : 'text-xs text-ink-faint'
  const summary = terminal?.status === 'completed'
    ? terminal.finalText
    : terminal?.status === 'failed'
      ? `${terminal.code}: ${terminal.message}`
      : terminal?.status === 'cancelled'
        ? t('chat.subAgents.cancelledSummary')
        : null
  const durationMs = runtime?.startedAtMs != null && runtime.endedAtMs != null
    ? Math.max(0, runtime.endedAtMs - runtime.startedAtMs)
    : null
  return { statusLabel, statusClass, summary, durationMs }
}

function ReadonlySubAgentTranscript({ messages }: { messages: RuntimeStoredMessage[] }) {
  const { t } = useTranslation()
  if (messages.length === 0) {
    return <p className="text-xs text-ink-faint">{t('chat.subAgents.emptyTranscript')}</p>
  }
  return (
    <div className="flex flex-col gap-4" data-readonly-sub-agent-transcript="true">
      {messages.map((message) => (
        <div key={message.id} className="min-w-0">
          {message.role === 'user' ? (
            <UserMessage parts={message.content} />
          ) : message.role === 'tool' ? (
            <ToolActivityList parts={message.content} />
          ) : (
            <AssistantMessage parts={message.content} />
          )}
        </div>
      ))}
    </div>
  )
}
