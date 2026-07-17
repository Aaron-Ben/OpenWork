import { useEffect, useMemo, useRef, useState } from 'react'
import { Sparkles, SquareTerminal } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { AnimatePresence } from 'motion/react'

import { ApprovalDialog } from '../components/chat/ApprovalDialog'
import { AssistantMessage } from '../components/chat/AssistantMessage'
import { ChatInput } from '../components/chat/ChatInput'
import {
  ConversationNavigator,
  getConversationTurns,
} from '../components/chat/ConversationNavigator'
import { ToolActivityList } from '../components/chat/ToolActivityList'
import { mergeToolMessages } from '../components/chat/toolActivity'
import { UserMessage } from '../components/chat/UserMessage'
import { RuntimeTracePanel } from '../components/trace/RuntimeTracePanel'
import { useActiveProvider } from '../stores/providerStore'
import {
  useActiveRuntimeMessages,
  useRuntimeSessionStore,
} from '../stores/runtimeSessionStore'

export function ChatView({ sessionId }: { sessionId: string | null }) {
  const active = useActiveProvider()
  const [draft, setDraft] = useState('')
  const [model, setModel] = useState('')
  const [selectedTrace, setSelectedTrace] = useState<{
    turnId: string
    providerToolCallId?: string
  } | null>(null)
  const scrollContainerRef = useRef<HTMLDivElement>(null)
  const messages = useActiveRuntimeMessages()
  const displayMessages = useMemo(() => mergeToolMessages(messages), [messages])
  const turns = useMemo(() => getConversationTurns(displayMessages), [displayMessages])
  const startTurn = useRuntimeSessionStore((state) => state.startTurn)
  const cancelActiveTurn = useRuntimeSessionStore((state) => state.cancelActiveTurn)
  const activeTurn = useRuntimeSessionStore((state) => state.activeTurn)
  // 是否正在发送 = 当前 session 有 in-flight 流式请求。
  const isSending = activeTurn?.sessionId === sessionId
  const modelOptions = active?.models.filter((item) => item.enabled) ?? []

  useEffect(() => {
    setModel(active?.models.find((item) => item.enabled)?.modelId ?? '')
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active?.id])

  useEffect(() => setSelectedTrace(null), [sessionId])

  async function send() {
    const text = draft.trim()
    if (!text || isSending || !sessionId) return
    setDraft('')
    await startTurn(sessionId, text)
  }

  return (
    <div className="grid h-full min-h-0 grid-rows-[minmax(0,1fr)_auto] bg-paper">
      <div className="relative min-h-0">
        <div ref={scrollContainerRef} className="h-full overflow-auto" aria-live="polite">
          {displayMessages.length === 0 ? (
            <EmptySessionHero active={!!active} hasSession={!!sessionId} />
          ) : (
            <div className="mx-auto flex w-full max-w-5xl flex-col gap-4 px-6 py-8 max-[560px]:px-4">
              {displayMessages.map((message) => {
                const content =
                  message.role === 'user' ? (
                    <UserMessage parts={message.parts} />
                  ) : message.role === 'tool' ? (
                    <ToolActivityList
                      parts={message.parts}
                      onOpenTrace={message.turnId
                        ? (providerToolCallId) => setSelectedTrace({
                            turnId: message.turnId!,
                            providerToolCallId,
                          })
                        : undefined}
                    />
                  ) : (
                    <AssistantMessage
                      parts={message.parts}
                      model={message.model}
                      isStreaming={message.isStreaming}
                      onOpenTrace={message.turnId
                        ? (providerToolCallId) => setSelectedTrace({
                            turnId: message.turnId!,
                            providerToolCallId,
                          })
                        : undefined}
                    />
                  )

                return (
                  <div
                    key={message.id}
                    data-message-id={message.id}
                    data-turn-id={message.role === 'user' ? message.id : undefined}
                  >
                    {content}
                  </div>
                )
              })}
            </div>
          )}
        </div>
        <ConversationNavigator turns={turns} scrollContainerRef={scrollContainerRef} />
        <AnimatePresence>
          {selectedTrace ? (
            <RuntimeTracePanel
              key={`${selectedTrace.turnId}:${selectedTrace.providerToolCallId ?? 'root'}`}
              turnId={selectedTrace.turnId}
              initialProviderCallId={selectedTrace.providerToolCallId}
              onClose={() => setSelectedTrace(null)}
            />
          ) : null}
        </AnimatePresence>
      </div>

      <ChatInput
        topContent={<ApprovalDialog />}
        model={model}
        modelOptions={modelOptions}
        value={draft}
        isSending={isSending}
        disabled={!sessionId}
        onValueChange={setDraft}
        onModelChange={setModel}
        onSubmit={() => void send()}
        onCancel={() => void cancelActiveTurn()}
      />
    </div>
  )
}

function EmptySessionHero({ active, hasSession }: { active: boolean; hasSession: boolean }) {
  const { t } = useTranslation()
  const title = hasSession ? t('chat.startConversation') : t('chat.createSession')
  const body = !hasSession
    ? t('chat.noSessionHelp')
    : active
      ? t('chat.readyHelp')
      : t('chat.providerRequired')

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
