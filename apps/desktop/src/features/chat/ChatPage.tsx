import { useEffect, useMemo, useRef, useState } from 'react'
import { LoaderCircle, Sparkles, SquareTerminal } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { AssistantMessage } from './components/AssistantMessage'
import { ChatInput } from './components/ChatInput'
import { ContextWindowDrawer } from './components/ContextWindowDrawer'
import {
  ConversationNavigator,
  getConversationTurns,
} from './components/ConversationNavigator'
import { ApprovalDialog } from './components/ApprovalDialog'
import { ToolActivityList } from './components/ToolActivityList'
import { UserMessage } from './components/UserMessage'
import { selectDefaultModel, useModelStore } from '../models/modelStore'
import type { RuntimeContextWindowInspection, RuntimeStoredMessage } from '../../bridge/compat'
import { coreCommands } from '../../bridge/commands'
import { TurnTraceDrawer } from '../traces/components/TurnTraceDrawer'
import { useSessionStore } from '../sessions/sessionStore'
import { EMPTY_RUNTIME_VIEW, useRuntimeStore } from './runtimeStore'
import { buildTranscript } from './transcript'
import { useTurnActions } from './useTurn'
import { contextUsageFromTrace, type ContextUsage } from './contextUsage'
import { useContextWindowStore } from '../../stores/contextWindowStore'
import { resolveErrorMessage } from '../../utils/commandError'
import { useNavigationStore } from '../../app/navigationStore'

const EMPTY_MESSAGES: RuntimeStoredMessage[] = []

export function isManualCompactionDraft(value: string): boolean {
  return value.trim().toLowerCase() === '/compact'
}

export function ChatPage({ sessionId }: { sessionId: string | null }) {
  const { t } = useTranslation()
  const [draft, setDraft] = useState('')
  const [isCompacting, setIsCompacting] = useState(false)
  const [compactionError, setCompactionError] = useState<string | null>(null)
  const [contextUsage, setContextUsage] = useState<ContextUsage | null>(null)
  const [contextInspectorOpen, setContextInspectorOpen] = useState(false)
  const [contextInspection, setContextInspection] = useState<RuntimeContextWindowInspection | null>(null)
  const [contextInspectionLoading, setContextInspectionLoading] = useState(false)
  const [contextInspectionError, setContextInspectionError] = useState<string | null>(null)
  const [contextInspectionRefresh, setContextInspectionRefresh] = useState(0)
  const [selectedTrace, setSelectedTrace] = useState<{ turnId: string; providerToolCallId?: string } | null>(null)
  const [highlightedMessageId, setHighlightedMessageId] = useState<string | null>(null)
  const scrollContainerRef = useRef<HTMLDivElement>(null)
  const highlightTimerRef = useRef<number | null>(null)
  const stickToBottomRef = useRef(true)
  const activeSessionIdRef = useRef(sessionId)
  activeSessionIdRef.current = sessionId
  const canonical = useSessionStore((state) => sessionId ? state.messagesBySession[sessionId] ?? EMPTY_MESSAGES : EMPTY_MESSAGES)
  const runtime = useRuntimeStore((state) => sessionId ? state.bySession[sessionId] ?? EMPTY_RUNTIME_VIEW : EMPTY_RUNTIME_VIEW)
  const session = useSessionStore((state) => sessionId ? state.summaries[sessionId] : undefined)
  const sessionError = useSessionStore((state) => state.error)
  const loadState = useSessionStore((state) => sessionId ? state.loadStateBySession[sessionId] : undefined)
  const reloadSession = useSessionStore((state) => state.reload)
  const providers = useModelStore((state) => state.providers)
  const contextWindowTokens = useContextWindowStore((state) => state.contextWindowTokens)
  const messageFocus = useNavigationStore((state) => state.messageFocus)
  const clearMessageFocus = useNavigationStore((state) => state.clearMessageFocus)
  const requestMessageFocus = useNavigationStore((state) => state.requestMessageFocus)
  const hasAvailableModel = selectDefaultModel(providers) !== null
  const { startTurn, cancelTurn, setPermissionMode } = useTurnActions(sessionId)
  const messages = useMemo(() => buildTranscript(canonical, runtime), [canonical, runtime])
  const contextBreakdown = useMemo(() => {
    if (!contextInspection) return null
    return {
      messagesTokens: contextInspection.budget.conversationTokens,
      systemPromptTokens: contextInspection.budget.systemContextTokens,
      systemToolsTokens: contextInspection.budget.toolSurfaceTokens,
    }
  }, [contextInspection])
  const turns = useMemo(() => getConversationTurns(messages), [messages])
  const isSending = runtime.phase !== 'idle'
  const finishedToolCallCount = runtime.orderedToolCallIds.reduce(
    (count, id) => count + (runtime.toolCalls[id]?.isError == null ? 0 : 1),
    0,
  )
  const contextInspectionRevision = [
    canonical.length,
    canonical[canonical.length - 1]?.id ?? '',
    runtime.turnId ?? '',
    runtime.phase,
    finishedToolCallCount,
  ].join(':')
  const sessionModel = useMemo(() => {
    if (!session?.defaultModelId) return null
    for (const provider of providers) {
      const model = provider.models.find(
        (item) => `model:${provider.id}:${item.modelId}` === session.defaultModelId,
      )
      if (model) return model
    }
    return {
      modelId: session.defaultModelId,
      displayName: session.defaultModelId,
      modelTier: 'plus' as const,
      enabled: true,
    }
  }, [providers, session?.defaultModelId])

  useEffect(() => {
    setSelectedTrace(null)
    setContextInspectorOpen(false)
    setContextInspection(null)
    setContextInspectionError(null)
    setCompactionError(null)
    setIsCompacting(false)
    setHighlightedMessageId(null)
    stickToBottomRef.current = true
    if (highlightTimerRef.current !== null) window.clearTimeout(highlightTimerRef.current)
  }, [sessionId])

  useEffect(() => {
    const container = scrollContainerRef.current
    if (container && stickToBottomRef.current) container.scrollTop = container.scrollHeight
  }, [messages])

  function handleTranscriptScroll() {
    const container = scrollContainerRef.current
    if (!container) return
    stickToBottomRef.current =
      container.scrollHeight - container.scrollTop - container.clientHeight < 80
  }

  useEffect(() => () => {
    if (highlightTimerRef.current !== null) window.clearTimeout(highlightTimerRef.current)
  }, [])

  useEffect(() => setContextUsage(null), [sessionId, contextWindowTokens])

  useEffect(() => {
    if (!messageFocus || messageFocus.sessionId !== sessionId) return
    const message = scrollContainerRef.current?.querySelector<HTMLElement>(
      `[data-message-id="${CSS.escape(messageFocus.messageId)}"]`,
    )
    if (!message) return
    message.scrollIntoView({ block: 'center' })
    setHighlightedMessageId(messageFocus.messageId)
    clearMessageFocus(messageFocus.requestId)
    if (highlightTimerRef.current !== null) window.clearTimeout(highlightTimerRef.current)
    highlightTimerRef.current = window.setTimeout(() => {
      setHighlightedMessageId((current) => current === messageFocus.messageId ? null : current)
      highlightTimerRef.current = null
    }, 2_500)
  }, [canonical, clearMessageFocus, messageFocus, sessionId])

  useEffect(() => {
    if (!sessionId) return
    let active = true
    const activeSessionId = sessionId
    const totalTokens = contextWindowTokens

    async function refreshContextUsage() {
      try {
        if (!isSending) {
          const inspection = await coreCommands.inspectContextWindow(activeSessionId)
          if (!active) return
          setContextInspection(inspection)
          setContextUsage({
            usedTokens: inspection.budget.estimatedInputTokens,
            totalTokens,
            estimated: true,
          })
          return
        }

        // 列表现在也包含无 Turn 的压缩 Trace，用量只能从有 Turn 的那条读取。
        const summaries = await coreCommands.listTraces(activeSessionId, 8)
        const latestTurnId = summaries.find((summary) => summary.turnId)?.turnId
        if (!latestTurnId) return
        const trace = await coreCommands.getTrace(latestTurnId)
        const usage = contextUsageFromTrace(trace, totalTokens)
        if (active && usage) setContextUsage(usage)
      } catch {
        // Context usage is observational and must never affect the chat runtime.
      }
    }

    void refreshContextUsage()
    const timer = isSending
      ? window.setInterval(() => void refreshContextUsage(), 1_500)
      : null
    return () => {
      active = false
      if (timer !== null) window.clearInterval(timer)
    }
  }, [canonical.length, contextWindowTokens, isSending, sessionId])

  useEffect(() => {
    if (!contextInspectorOpen || !sessionId) return
    let active = true
    setContextInspectionLoading(true)
    setContextInspectionError(null)
    void coreCommands.inspectContextWindow(sessionId)
      .then((value) => {
        if (!active) return
        setContextInspection(value)
        setContextUsage({
          usedTokens: value.budget.estimatedInputTokens,
          totalTokens: contextWindowTokens,
          estimated: true,
        })
      })
      .catch((reason) => {
        if (active) setContextInspectionError(resolveErrorMessage(reason))
      })
      .finally(() => {
        if (active) setContextInspectionLoading(false)
      })
    return () => {
      active = false
    }
  }, [contextInspectorOpen, contextInspectionRefresh, contextInspectionRevision, contextWindowTokens, sessionId])

  async function send() {
    const text = draft.trim()
    if (!text || isSending || isCompacting || !sessionId) return
    if (isManualCompactionDraft(text)) {
      await runCompaction()
      return
    }
    setDraft('')
    stickToBottomRef.current = true
    const accepted = await startTurn(text)
    if (!accepted) setDraft(text)
  }

  async function runCompaction() {
    if (!sessionId || isSending || isCompacting) return
    const targetSessionId = sessionId
    setDraft('')
    setCompactionError(null)
    setIsCompacting(true)
    try {
      await coreCommands.compactConversation(targetSessionId)
      if (activeSessionIdRef.current !== targetSessionId) return
      setContextInspectionError(null)
      setContextInspectorOpen(true)
      setContextInspectionRefresh((value) => value + 1)
    } catch (reason) {
      if (activeSessionIdRef.current !== targetSessionId) return
      setCompactionError(resolveErrorMessage(reason))
      setDraft('/compact')
    } finally {
      if (activeSessionIdRef.current === targetSessionId) setIsCompacting(false)
    }
  }

  async function undoFileChanges(changeIds: string[]) {
    if (!sessionId) return
    await coreCommands.undoFileChanges(sessionId, changeIds)
    const reloaded = await reloadSession(sessionId)
    if (!reloaded) throw new Error('File changes were undone, but the conversation could not be refreshed')
  }

  async function reapplyFileChanges(changeIds: string[]) {
    if (!sessionId) return
    await coreCommands.reapplyFileChanges(sessionId, changeIds)
    const reloaded = await reloadSession(sessionId)
    if (!reloaded) throw new Error('File changes were reapplied, but the conversation could not be refreshed')
  }

  return (
    <div className="grid h-full min-h-0 grid-rows-[minmax(0,1fr)_auto] bg-paper">
      <div className="relative min-h-0">
        <div ref={scrollContainerRef} className="h-full overflow-auto" onScroll={handleTranscriptScroll}>
          {messages.length === 0 ? (
            sessionId && loadState === 'loading' ? (
              <div className="grid min-h-full place-items-center px-6 py-14" role="status">
                <span className="inline-flex items-center gap-2 text-sm text-ink-faint">
                  <LoaderCircle size={15} className="animate-spin" />
                  {t('chat.loading')}
                </span>
              </div>
            ) : sessionId && loadState === 'error' ? (
              <div className="grid min-h-full place-items-center px-6 py-14">
                <div className="text-center">
                  <p className="text-sm text-status-danger-ink" role="alert">
                    {sessionError ?? t('chat.loadFailed')}
                  </p>
                  <button
                    type="button"
                    className="mt-3 rounded-lg border border-line bg-paper px-4 py-2 text-sm text-ink transition hover:bg-paper-hover"
                    onClick={() => void reloadSession(sessionId)}
                  >
                    {t('chat.retry')}
                  </button>
                </div>
              </div>
            ) : (
              <EmptySessionHero
                hasAvailableModel={hasAvailableModel}
                hasSession={Boolean(sessionId)}
              />
            )
          ) : (
            <div className="mx-auto flex w-full max-w-4xl flex-col gap-4 px-6 py-8 max-[560px]:px-4">
              {messages.map((message) => {
                const openTrace = message.turnId
                  ? (providerToolCallId?: string) => setSelectedTrace({ turnId: message.turnId!, providerToolCallId })
                  : undefined
                const fileChangePresentation = message.fileChangePresentation ?? 'activity'
                return (
                  <div
                    key={message.id}
                    data-message-id={message.id}
                    data-turn-id={message.role === 'user' ? message.turnId ?? message.id : undefined}
                    className={highlightedMessageId === message.id
                      ? 'rounded-xl bg-clay-soft ring-2 ring-clay/45 transition-colors'
                      : 'rounded-xl transition-colors'}
                  >
                    {message.role === 'user' ? (
                      <UserMessage parts={message.parts} />
                    ) : message.role === 'tool' ? (
                      <ToolActivityList
                        parts={message.parts}
                        onOpenTrace={openTrace}
                        onUndoFileChanges={undoFileChanges}
                        onReapplyFileChanges={reapplyFileChanges}
                        fileChangePresentation={fileChangePresentation}
                      />
                    ) : (
                      <AssistantMessage
                        parts={message.parts}
                        model={message.model}
                        isStreaming={message.isStreaming}
                        isCompacting={message.isCompacting}
                        onOpenTrace={openTrace}
                        onUndoFileChanges={undoFileChanges}
                        onReapplyFileChanges={reapplyFileChanges}
                        fileChangePresentation={fileChangePresentation}
                      />
                    )}
                  </div>
                )
              })}
            </div>
          )}
        </div>
        <ConversationNavigator turns={turns} scrollContainerRef={scrollContainerRef} />
        {selectedTrace ? (
          <TurnTraceDrawer
            source={{ kind: 'turn', turnId: selectedTrace.turnId }}
            initialProviderCallId={selectedTrace.providerToolCallId}
            onOpenMessage={(targetSessionId, messageId) => {
              requestMessageFocus(targetSessionId, messageId)
              setSelectedTrace(null)
            }}
            onClose={() => setSelectedTrace(null)}
          />
        ) : null}
        {contextInspectorOpen && sessionId ? (
          <ContextWindowDrawer
            sessionId={sessionId}
            inspection={contextInspection}
            contextWindowTokens={contextWindowTokens}
            highlightedTurnId={runtime.turnId}
            loading={contextInspectionLoading}
            error={contextInspectionError}
            refreshToken={contextInspectionRefresh}
            onRefresh={() => setContextInspectionRefresh((value) => value + 1)}
            onClose={() => setContextInspectorOpen(false)}
          />
        ) : null}
      </div>

      <div>
        {runtime.syncState !== 'current' ? (
          <p className="mx-auto mb-2 max-w-4xl px-6 text-xs text-status-warning-ink max-[560px]:px-4">
            {runtime.syncState === 'resyncing' ? t('chat.syncResyncing') : t('chat.syncStale')}
          </p>
        ) : null}
        {isCompacting || runtime.phase === 'compacting' ? (
          <p
            className="mx-auto mb-2 flex max-w-4xl items-center gap-2 px-6 text-xs text-ink-faint max-[560px]:px-4"
            role="status"
            data-compacting-indicator="true"
          >
            <LoaderCircle size={13} className="animate-spin" />
            {t('chat.commands.compacting')}
          </p>
        ) : null}
        {compactionError || runtime.error || sessionError ? (
          <p className="mx-auto mb-2 max-w-4xl px-6 text-xs text-status-danger-ink max-[560px]:px-4" role="alert">
            {compactionError ?? runtime.error ?? sessionError}
          </p>
        ) : null}
        <ChatInput
          topContent={<ApprovalDialog sessionId={sessionId} />}
          model={sessionModel?.modelId ?? ''}
          modelOptions={sessionModel ? [sessionModel] : []}
          modelSelectionLocked
          permissionMode={runtime.permissionMode}
          contextUsage={contextUsage}
          contextBreakdown={contextBreakdown}
          contextInspectorOpen={contextInspectorOpen}
          value={draft}
          isSending={isSending}
          isCompacting={isCompacting}
          disabled={!sessionId || !sessionModel}
          onValueChange={setDraft}
          onModelChange={() => undefined}
          onPermissionModeChange={(mode) => void setPermissionMode(mode)}
          onSubmit={() => void send()}
          onCancel={() => void cancelTurn()}
          onInspectContext={sessionId ? () => setContextInspectorOpen(true) : undefined}
          onSlashCommand={(command) => {
            if (command === 'compact') void runCompaction()
          }}
        />
      </div>
    </div>
  )
}

function EmptySessionHero({
  hasAvailableModel,
  hasSession,
}: {
  hasAvailableModel: boolean
  hasSession: boolean
}) {
  const { t } = useTranslation()
  const title = hasSession ? t('chat.startConversation') : t('chat.createSession')
  const body = !hasAvailableModel
    ? t('chat.providerRequired')
    : !hasSession
      ? t('chat.noSessionHelp')
      : t('chat.readyHelp')
  return (
    <div className="grid min-h-full place-items-center px-6 py-14 text-center">
      <div className="mb-20 max-w-md">
        <div className="mx-auto mb-8 grid size-20 place-items-center text-clay">
          <div className="relative">
            <SquareTerminal size={62} strokeWidth={2.2} className="text-ink" />
            <Sparkles size={20} className="absolute -right-3 top-1 text-clay" />
          </div>
        </div>
        <h2 className="text-3xl font-semibold tracking-normal text-ink">{title}</h2>
        <p className="mt-4 text-base leading-7 text-ink-soft">{body}</p>
      </div>
    </div>
  )
}
