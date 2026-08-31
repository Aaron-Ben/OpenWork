import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { LoaderCircle, Sparkles, SquareTerminal } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { ChatInput } from './components/ChatInput'
import { ContextWindowDrawer } from './components/ContextWindowDrawer'
import {
  ConversationNavigator,
  getConversationTurns,
} from './components/ConversationNavigator'
import { ApprovalDialog } from './components/ApprovalDialog'
import { FileChangeReviewDrawer } from './components/FileChangeReviewDrawer'
import type { FileChangeView } from './components/FileDiffPanel'
import { TranscriptMessage } from './components/TranscriptMessage'
import { transcriptGap } from './transcriptSpacing'
import { selectDefaultModel, useModelStore } from '@/features/models/modelStore'
import type {
  RuntimeContextWindowInspection,
  RuntimeSkillInput,
  RuntimeSkillSummary,
  RuntimeStoredMessage,
  RuntimeTurnPlan,
} from '@/bridge/compat'
import { coreCommands } from '@/bridge/commands'
import { TurnTraceDrawer } from '@/features/traces/components/TurnTraceDrawer'
import { useSessionStore } from '@/features/sessions/sessionStore'
import { EMPTY_RUNTIME_VIEW, useRuntimeStore } from './runtimeStore'
import { buildTranscript } from './transcript'
import { useTurnActions } from './useTurn'
import { contextUsageFromTrace, type ContextUsage } from './contextUsage'
import { resolveErrorMessage } from '@/lib/commandError'
import { useNavigationStore } from '@/app/navigationStore'

const EMPTY_MESSAGES: RuntimeStoredMessage[] = []
const EMPTY_PLANS: RuntimeTurnPlan[] = []

export function isManualCompactionDraft(value: string): boolean {
  return value.trim().toLowerCase() === '/compact'
}

export function shouldClearAcceptedDraft(
  activeSessionId: string | null,
  submittedSessionId: string,
  currentRevision: number,
  submittedRevision: number,
): boolean {
  return activeSessionId === submittedSessionId && currentRevision === submittedRevision
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
  const [reviewChanges, setReviewChanges] = useState<FileChangeView[] | null>(null)
  const [highlightedMessageId, setHighlightedMessageId] = useState<string | null>(null)
  const [availableSkills, setAvailableSkills] = useState<RuntimeSkillSummary[]>([])
  const scrollContainerRef = useRef<HTMLDivElement>(null)
  const highlightTimerRef = useRef<number | null>(null)
  const skillRefreshSequenceRef = useRef(0)
  const draftRevisionRef = useRef(0)
  const draftSessionIdRef = useRef(sessionId)
  if (draftSessionIdRef.current !== sessionId) {
    draftSessionIdRef.current = sessionId
    draftRevisionRef.current += 1
  }
  const stickToBottomRef = useRef(true)
  const activeSessionIdRef = useRef(sessionId)
  activeSessionIdRef.current = sessionId
  const canonical = useSessionStore((state) => sessionId ? state.messagesBySession[sessionId] ?? EMPTY_MESSAGES : EMPTY_MESSAGES)
  const plans = useSessionStore((state) => sessionId ? state.plansBySession[sessionId] ?? EMPTY_PLANS : EMPTY_PLANS)
  const runtime = useRuntimeStore((state) => sessionId ? state.bySession[sessionId] ?? EMPTY_RUNTIME_VIEW : EMPTY_RUNTIME_VIEW)
  const session = useSessionStore((state) => sessionId ? state.summaries[sessionId] : undefined)
  const sessionError = useSessionStore((state) => state.error)
  const loadState = useSessionStore((state) => sessionId ? state.loadStateBySession[sessionId] : undefined)
  const reloadSession = useSessionStore((state) => state.reload)
  const providers = useModelStore((state) => state.providers)
  const messageFocus = useNavigationStore((state) => state.messageFocus)
  const clearMessageFocus = useNavigationStore((state) => state.clearMessageFocus)
  const requestMessageFocus = useNavigationStore((state) => state.requestMessageFocus)
  const hasAvailableModel = selectDefaultModel(providers) !== null
  const { startTurn, cancelTurn, setPermissionMode } = useTurnActions(sessionId)
  const messages = useMemo(
    () => buildTranscript(canonical, runtime, plans),
    [canonical, runtime, plans],
  )
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
  const contextWindowTokens = sessionModel?.capabilities?.contextWindowTokens ?? null

  const refreshSkills = useCallback(async () => {
    const requestSequence = skillRefreshSequenceRef.current + 1
    skillRefreshSequenceRef.current = requestSequence
    try {
      const discovery = await coreCommands.listSkills()
      if (skillRefreshSequenceRef.current === requestSequence) {
        setAvailableSkills(discovery.skills.filter((skill) => !skill.disabled))
      }
    } catch {
      // Keep the last successful snapshot so a transient host error does not
      // make the picker look as though every Skill was removed.
    }
  }, [])

  useEffect(() => {
    void refreshSkills()
  }, [refreshSkills, sessionId])

  useEffect(() => {
    setSelectedTrace(null)
    setReviewChanges(null)
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

  const handleTranscriptScroll = useCallback(() => {
    const container = scrollContainerRef.current
    if (!container) return
    stickToBottomRef.current =
      container.scrollHeight - container.scrollTop - container.clientHeight < 80
  }, [])

  useEffect(() => () => {
    if (highlightTimerRef.current !== null) window.clearTimeout(highlightTimerRef.current)
  }, [])

  useEffect(() => setContextUsage(null), [sessionId, contextWindowTokens])

  useEffect(() => {
    if (!messageFocus || messageFocus.sessionId !== sessionId) return
    const escapedMessageId = CSS.escape(messageFocus.messageId)
    const message = scrollContainerRef.current?.querySelector<HTMLElement>(
      `[data-message-id="${escapedMessageId}"], [data-source-message-ids~="${escapedMessageId}"]`,
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
    if (contextWindowTokens === null) {
      setContextUsage(null)
      return
    }
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
          totalTokens: value.budget.contextWindowTokens,
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
  }, [contextInspectorOpen, contextInspectionRefresh, contextInspectionRevision, sessionId])

  async function send(skills: RuntimeSkillInput[] = []) {
    const submittedRevision = draftRevisionRef.current
    const text = draft.trim()
    if (!text || isSending || isCompacting || !sessionId) return
    if (isManualCompactionDraft(text)) {
      await runCompaction()
      return
    }
    const submittedSessionId = sessionId
    stickToBottomRef.current = true
    const accepted = await startTurn(text, skills)
    if (accepted && shouldClearAcceptedDraft(
      activeSessionIdRef.current,
      submittedSessionId,
      draftRevisionRef.current,
      submittedRevision,
    )) {
      setDraft('')
    }
  }

  function updateDraft(value: string) {
    draftRevisionRef.current += 1
    setDraft(value)
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

  const undoFileChanges = useCallback(async (changeIds: string[]) => {
    if (!sessionId) return
    await coreCommands.undoFileChanges(sessionId, changeIds)
    const reloaded = await reloadSession(sessionId)
    if (!reloaded) throw new Error('File changes were undone, but the conversation could not be refreshed')
  }, [reloadSession, sessionId])

  const reapplyFileChanges = useCallback(async (changeIds: string[]) => {
    if (!sessionId) return
    await coreCommands.reapplyFileChanges(sessionId, changeIds)
    const reloaded = await reloadSession(sessionId)
    if (!reloaded) throw new Error('File changes were reapplied, but the conversation could not be refreshed')
  }, [reloadSession, sessionId])

  const openTrace = useCallback((turnId: string, providerToolCallId?: string) => {
    setSelectedTrace({ turnId, providerToolCallId })
  }, [])

  const reviewFileChanges = useCallback((changes: FileChangeView[]) => {
    setReviewChanges(changes)
  }, [])

  return (
    /*
      两个行元素都要 min-w-0。grid item 的自动最小尺寸取内容的 min-content，
      而下面的 max-w-4xl 会把 min-content 顶到 896px：中间栏窄于这个值时
      （三栏布局下很常见），列宽被撑破，超出的正文被外层 overflow-hidden 切掉。
    */
    <div className="grid h-full min-h-0 min-w-0 grid-rows-[minmax(0,1fr)_auto] bg-paper">
      <div className="relative min-h-0 min-w-0">
        <div
          ref={scrollContainerRef}
          className="h-full overflow-auto"
          style={{ contain: 'layout paint style' }}
          onScroll={handleTranscriptScroll}
        >
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
            /*
              容器不设 gap：相邻消息之间的间距由 transcriptGap 按"上下两个块是什么"决定，
              否则一串连续的工具行会被 provider 的分包方式切成远近不等的簇。
            */
            <div className="mx-auto flex w-full max-w-4xl flex-col px-6 py-8 max-[560px]:px-4">
              {messages.map((message, index) => (
                <TranscriptMessage
                  key={message.id}
                  message={message}
                  highlighted={highlightedMessageId === message.id
                    || message.sourceMessageIds?.includes(highlightedMessageId ?? '') === true}
                  gap={transcriptGap(messages[index - 1], message)}
                  onOpenTrace={openTrace}
                  onUndoFileChanges={undoFileChanges}
                  onReapplyFileChanges={reapplyFileChanges}
                  onReviewFileChanges={reviewFileChanges}
                  workspaceRoot={session?.workingDirectory}
                />
              ))}
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
        {reviewChanges ? (
          <FileChangeReviewDrawer
            changes={reviewChanges}
            workspaceRoot={session?.workingDirectory}
            onClose={() => setReviewChanges(null)}
          />
        ) : null}
        {contextInspectorOpen && sessionId ? (
          <ContextWindowDrawer
            sessionId={sessionId}
            inspection={contextInspection}
            highlightedTurnId={runtime.turnId}
            loading={contextInspectionLoading}
            error={contextInspectionError}
            refreshToken={contextInspectionRefresh}
            onRefresh={() => setContextInspectionRefresh((value) => value + 1)}
            onClose={() => setContextInspectorOpen(false)}
          />
        ) : null}
      </div>

      <div className="min-w-0">
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
          key={sessionId ?? 'no-session'}
          topContent={(
            <ApprovalDialog
              sessionId={sessionId}
              workspaceRoot={session?.workingDirectory}
            />
          )}
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
          skills={availableSkills}
          onValueChange={updateDraft}
          onModelChange={() => undefined}
          onPermissionModeChange={(mode) => void setPermissionMode(mode)}
          onSubmit={(skills) => void send(skills)}
          onCancel={() => void cancelTurn()}
          onInspectContext={sessionId ? () => setContextInspectorOpen(true) : undefined}
          onRefreshSkills={refreshSkills}
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
