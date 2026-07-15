import { useEffect, useMemo, useRef, useState } from 'react'
import { Sparkles, SquareTerminal } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { AnimatePresence } from 'motion/react'

import { sessionsApi } from '../api/sessions'
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
import { TurnTracePanel } from '../components/trace/TurnTracePanel'
import { useActiveProvider } from '../stores/providerStore'
import { useSessionStore, useActiveSessionMessages } from '../stores/sessionStore'
import { DEFAULT_APPROVAL_POLICY } from '../type/chat'
import type { TurnTraceSummary } from '../type/trace'

export function ChatView({ sessionId }: { sessionId: string | null }) {
  const active = useActiveProvider()
  const [draft, setDraft] = useState('')
  const [model, setModel] = useState('')
  const [selectedTraceTurnId, setSelectedTraceTurnId] = useState<string | null>(null)
  const [traceSummaries, setTraceSummaries] = useState<Record<string, TurnTraceSummary>>({})
  const scrollContainerRef = useRef<HTMLDivElement>(null)
  const messages = useActiveSessionMessages()
  const displayMessages = useMemo(() => mergeToolMessages(messages), [messages])
  const turns = useMemo(() => getConversationTurns(displayMessages), [displayMessages])
  const pushUserMessage = useSessionStore((state) => state.pushUserMessage)
  const ensureStreamingItem = useSessionStore((state) => state.ensureStreamingItem)
  const finishStreaming = useSessionStore((state) => state.finishStreaming)
  const setActiveStream = useSessionStore((state) => state.setActiveStream)
  const cancelActiveStream = useSessionStore((state) => state.cancelActiveStream)
  const activeStream = useSessionStore((state) => state.activeStream)
  // 是否正在发送 = 当前 session 有 in-flight 流式请求。
  const isSending = activeStream?.sessionId === sessionId
  const modelOptions = active?.models.filter((item) => item.enabled) ?? []

  useEffect(() => {
    setModel(active?.models.find((item) => item.enabled)?.modelId ?? '')
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active?.id])

  useEffect(() => {
    setSelectedTraceTurnId(null)
    if (!sessionId) {
      setTraceSummaries({})
      return
    }
    let current = true
    void sessionsApi.traceSession(sessionId).then((summaries) => {
      if (!current) return
      setTraceSummaries(
        Object.fromEntries(summaries.map((summary) => [summary.turnId, summary])),
      )
    }).catch(() => {
      if (current) setTraceSummaries({})
    })
    return () => {
      current = false
    }
  }, [sessionId, activeStream?.requestId])

  async function send() {
    const text = draft.trim()
    if (!text || isSending || !active || !model || !sessionId) return
    const requestId = crypto.randomUUID()
    setDraft('')
    // 乐观:立即显示用户消息 + 临时 assistant item + 标记 in-flight。
    pushUserMessage(sessionId, requestId, text)
    ensureStreamingItem(sessionId, requestId, model)
    setActiveStream({ sessionId, requestId })
    try {
      await sessionsApi.chatGenerateStream({
        requestId,
        sessionId,
        providerId: active.id,
        model,
        userText: text,
        approvalPolicy: DEFAULT_APPROVAL_POLICY,
      })
    } catch {
      // 错误文本由 listener 的 error event 累积到 store;这里仅兜底结束流式态。
    } finally {
      // cancelled/done 事件会清 activeStream;这里兜底(防事件丢失)。
      setActiveStream(null)
      finishStreaming(sessionId, requestId)
    }
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
                    <ToolActivityList parts={message.parts} />
                  ) : (
                    <AssistantMessage
                      parts={message.parts}
                      model={message.model}
                      isStreaming={message.isStreaming}
                      traceSummary={message.turnId ? traceSummaries[message.turnId] : undefined}
                      onOpenTrace={message.turnId ? () => setSelectedTraceTurnId(message.turnId!) : undefined}
                    />
                  )

                return (
                  <div
                    key={message.id}
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
          {selectedTraceTurnId ? (
            <TurnTracePanel
              key={selectedTraceTurnId}
              turnId={selectedTraceTurnId}
              onClose={() => setSelectedTraceTurnId(null)}
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
        disabled={!active || !sessionId}
        onValueChange={setDraft}
        onModelChange={setModel}
        onSubmit={() => void send()}
        onCancel={() => void cancelActiveStream()}
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
