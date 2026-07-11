import { useEffect, useState } from 'react'
import { Sparkles, SquareTerminal } from 'lucide-react'

import { sessionsApi } from '../api/sessions'
import { ApprovalDialog } from '../components/chat/ApprovalDialog'
import { AssistantMessage } from '../components/chat/AssistantMessage'
import { ChatInput } from '../components/chat/ChatInput'
import { ToolResultView } from '../components/chat/ToolResultView'
import { UserMessage } from '../components/chat/UserMessage'
import { useActiveProvider } from '../stores/providerStore'
import { useApprovalStore } from '../stores/approvalStore'
import { useSessionStore, useActiveSessionMessages } from '../stores/sessionStore'

export function ChatView({ sessionId }: { sessionId: string | null }) {
  const active = useActiveProvider()
  const [draft, setDraft] = useState('')
  const [model, setModel] = useState('')
  const messages = useActiveSessionMessages()
  const hasPendingApproval = useApprovalStore((state) => state.pending.length > 0)
  const pushUserMessage = useSessionStore((state) => state.pushUserMessage)
  const ensureStreamingItem = useSessionStore((state) => state.ensureStreamingItem)
  const finishStreaming = useSessionStore((state) => state.finishStreaming)
  const setActiveStream = useSessionStore((state) => state.setActiveStream)
  const cancelActiveStream = useSessionStore((state) => state.cancelActiveStream)
  const activeStream = useSessionStore((state) => state.activeStream)
  // 是否正在发送 = 当前 session 有 in-flight 流式请求。
  const isSending = activeStream?.sessionId === sessionId
  const modelOptions = active?.models.filter((item) => item.enabled).map((item) => item.modelId) ?? []

  useEffect(() => {
    setModel(active?.models.find((item) => item.enabled)?.modelId ?? '')
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active?.id])

  async function send() {
    const text = draft.trim()
    if (!text || isSending || !active || !model || !sessionId) return
    const requestId = crypto.randomUUID()
    setDraft('')
    // 乐观:立即显示用户消息 + 临时 assistant item + 标记 in-flight。
    pushUserMessage(sessionId, text)
    ensureStreamingItem(sessionId, requestId, model)
    setActiveStream({ sessionId, requestId })
    try {
      await sessionsApi.chatGenerateStream({
        requestId,
        sessionId,
        providerId: active.id,
        model,
        userText: text,
        approvalPolicy: 'untrusted',
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
      <div className="min-h-0 overflow-auto" aria-live="polite">
        {messages.length === 0 && !hasPendingApproval ? (
          <EmptySessionHero active={!!active} hasSession={!!sessionId} />
        ) : (
          <div className="mx-auto flex w-full max-w-5xl flex-col gap-4 px-6 py-8 max-[560px]:px-4">
            {messages.map((message) =>
              message.role === 'user' ? (
                <UserMessage key={message.id} parts={message.parts} />
              ) : message.role === 'tool' ? (
                message.parts.map((part) =>
                  part.type === 'tool_result' ? (
                    <ToolResultView key={part.id} part={part} />
                  ) : null,
                )
              ) : (
                <AssistantMessage
                  key={message.id}
                  parts={message.parts}
                  model={message.model}
                  isStreaming={message.isStreaming}
                />
              ),
            )}
            <ApprovalDialog />
          </div>
        )}
      </div>

      <ChatInput
        activeProviderName={active?.name ?? 'No provider'}
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
  const title = hasSession ? '开始对话' : '新建会话'
  const body = !hasSession
    ? '点击侧栏的「新建会话」按钮,开始使用 OpenWork。'
    : active
      ? '开始一个新的编码会话。OpenWork 已准备好帮你构建、调试和梳理项目。'
      : '先在 Settings 中配置并启用一个云端 Provider,然后开始新的编码会话。'

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
