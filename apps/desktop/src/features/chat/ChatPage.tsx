import { useEffect, useMemo, useRef, useState } from 'react'
import { Sparkles, SquareTerminal } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { AssistantMessage } from './components/AssistantMessage'
import { ChatInput } from './components/ChatInput'
import {
  ConversationNavigator,
  getConversationTurns,
} from './components/ConversationNavigator'
import { ApprovalDialog } from './components/ApprovalDialog'
import { ToolActivityList } from './components/ToolActivityList'
import { UserMessage } from './components/UserMessage'
import { selectDefaultModel, useModelStore } from '../models/modelStore'
import type { RuntimeStoredMessage } from '../../bridge/compat'
import { TurnTraceDrawer } from '../traces/components/TurnTraceDrawer'
import { useSessionStore } from '../sessions/sessionStore'
import { EMPTY_RUNTIME_VIEW, useRuntimeStore } from './runtimeStore'
import { buildTranscript } from './transcript'
import { useTurnActions } from './useTurn'

const EMPTY_MESSAGES: RuntimeStoredMessage[] = []

export function ChatPage({ sessionId }: { sessionId: string | null }) {
  const { t } = useTranslation()
  const [draft, setDraft] = useState('')
  const [selectedTrace, setSelectedTrace] = useState<{ turnId: string; providerToolCallId?: string } | null>(null)
  const scrollContainerRef = useRef<HTMLDivElement>(null)
  const canonical = useSessionStore((state) => sessionId ? state.messagesBySession[sessionId] ?? EMPTY_MESSAGES : EMPTY_MESSAGES)
  const runtime = useRuntimeStore((state) => sessionId ? state.bySession[sessionId] ?? EMPTY_RUNTIME_VIEW : EMPTY_RUNTIME_VIEW)
  const session = useSessionStore((state) => sessionId ? state.summaries[sessionId] : undefined)
  const sessionError = useSessionStore((state) => state.error)
  const providers = useModelStore((state) => state.providers)
  const hasAvailableModel = selectDefaultModel(providers) !== null
  const { startTurn, cancelTurn } = useTurnActions(sessionId)
  const messages = useMemo(() => buildTranscript(canonical, runtime), [canonical, runtime])
  const turns = useMemo(() => getConversationTurns(messages), [messages])
  const isSending = runtime.phase !== 'idle'
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

  useEffect(() => setSelectedTrace(null), [sessionId])

  async function send() {
    const text = draft.trim()
    if (!text || isSending || !sessionId) return
    setDraft('')
    const accepted = await startTurn(text)
    if (!accepted) setDraft(text)
  }

  return (
    <div className="grid h-full min-h-0 grid-rows-[minmax(0,1fr)_auto] bg-paper">
      <div className="relative min-h-0">
        <div ref={scrollContainerRef} className="h-full overflow-auto" aria-live="polite">
          {messages.length === 0 ? (
            <EmptySessionHero
              hasAvailableModel={hasAvailableModel}
              hasSession={Boolean(sessionId)}
            />
          ) : (
            <div className="mx-auto flex w-full max-w-5xl flex-col gap-4 px-6 py-8 max-[560px]:px-4">
              {messages.map((message) => {
                const openTrace = message.turnId
                  ? (providerToolCallId?: string) => setSelectedTrace({ turnId: message.turnId!, providerToolCallId })
                  : undefined
                return (
                  <div
                    key={message.id}
                    data-message-id={message.id}
                    data-turn-id={message.role === 'user' ? message.turnId ?? message.id : undefined}
                  >
                    {message.role === 'user' ? (
                      <UserMessage parts={message.parts} />
                    ) : message.role === 'tool' ? (
                      <ToolActivityList parts={message.parts} onOpenTrace={openTrace} />
                    ) : (
                      <AssistantMessage
                        parts={message.parts}
                        model={message.model}
                        isStreaming={message.isStreaming}
                        onOpenTrace={openTrace}
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
            turnId={selectedTrace.turnId}
            initialProviderCallId={selectedTrace.providerToolCallId}
            onClose={() => setSelectedTrace(null)}
          />
        ) : null}
      </div>

      <div>
        {runtime.syncState !== 'current' ? (
          <p className="mx-auto mb-2 max-w-5xl px-6 text-xs text-status-warning-ink">
            {runtime.syncState === 'resyncing' ? t('chat.syncResyncing') : t('chat.syncStale')}
          </p>
        ) : null}
        {runtime.error || sessionError ? (
          <p className="mx-auto mb-2 max-w-5xl px-6 text-xs text-status-danger-ink" role="alert">
            {runtime.error ?? sessionError}
          </p>
        ) : null}
        <ChatInput
          topContent={<ApprovalDialog sessionId={sessionId} />}
          model={sessionModel?.modelId ?? ''}
          modelOptions={sessionModel ? [sessionModel] : []}
          modelSelectionLocked
          value={draft}
          isSending={isSending}
          disabled={!sessionId || !sessionModel}
          onValueChange={setDraft}
          onModelChange={() => undefined}
          onSubmit={() => void send()}
          onCancel={() => void cancelTurn()}
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
