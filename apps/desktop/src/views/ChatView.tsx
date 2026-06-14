import { useEffect, useRef, useState } from 'react'
import { Sparkles, SquareTerminal } from 'lucide-react'

import { providersApi } from '../api/providers'
import { AssistantMessage } from '../components/chat/AssistantMessage'
import { ChatInput } from '../components/chat/ChatInput'
import { UserMessage } from '../components/chat/UserMessage'
import { useActiveProvider } from '../stores/providerStore'
import type { ChatItem } from '../type/chat'

const WELCOME: ChatItem = {
  id: 'welcome',
  role: 'assistant',
  content: 'Add a cloud provider in Providers, activate it, then start a conversation.',
}

export function ChatView({ activeId }: { activeId: string | null }) {
  const active = useActiveProvider()
  const [draft, setDraft] = useState('')
  const [model, setModel] = useState('')
  const [messages, setMessages] = useState<ChatItem[]>([WELCOME])
  const [isSending, setIsSending] = useState(false)
  const currentRequestIdRef = useRef<string | null>(null)
  const visibleMessages = messages.filter((message) => message.id !== 'welcome')

  useEffect(() => {
    setModel(active?.models[0] ?? '')
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeId])

  useEffect(() => {
    let disposed = false
    let unlisten: (() => void) | null = null

    void providersApi.listenToChatStream((payload) => {
      if (disposed || payload.requestId !== currentRequestIdRef.current) return

      if (payload.event === 'text_delta' && payload.delta) {
        setMessages((current) =>
          current.map((message) =>
            message.id === payload.requestId
              ? { ...message, content: `${message.content}${payload.delta}` }
              : message,
          ),
        )
        return
      }

      if (payload.event === 'reasoning_delta' && payload.delta) {
        setMessages((current) =>
          current.map((message) =>
            message.id === payload.requestId
              ? { ...message, reasoningText: `${message.reasoningText ?? ''}${payload.delta}` }
              : message,
          ),
        )
        return
      }

      if (payload.event === 'tool_call_start' && payload.toolCallId) {
        setMessages((current) =>
          current.map((message) => {
            if (message.id !== payload.requestId) return message
            const toolCalls = [...(message.toolCalls ?? [])]
            if (!toolCalls.some((toolCall) => toolCall.id === payload.toolCallId)) {
              toolCalls.push({
                id: payload.toolCallId!,
                toolName: payload.toolName ?? '',
                partialInput: '',
                result: null,
              })
            }
            return { ...message, toolCalls }
          }),
        )
        return
      }

      if (payload.event === 'tool_call_delta' && payload.toolCallId) {
        setMessages((current) =>
          current.map((message) => {
            if (message.id !== payload.requestId) return message
            const toolCalls = (message.toolCalls ?? []).map((toolCall) =>
              toolCall.id === payload.toolCallId
                ? { ...toolCall, partialInput: toolCall.partialInput + (payload.partialInput ?? '') }
                : toolCall,
            )
            return { ...message, toolCalls }
          }),
        )
        return
      }

      if (payload.event === 'tool_result' && payload.toolCallId) {
        setMessages((current) =>
          current.map((message) => {
            if (message.id !== payload.requestId) return message
            const toolCalls = (message.toolCalls ?? []).map((toolCall) =>
              toolCall.id === payload.toolCallId
                ? {
                    ...toolCall,
                    result: {
                      output: payload.toolOutput ?? '',
                      isError: payload.isError ?? false,
                    },
                  }
                : toolCall,
            )
            return { ...message, toolCalls }
          }),
        )
        return
      }

      if (payload.event === 'error') {
        setMessages((current) =>
          current.map((message) =>
            message.id === payload.requestId
              ? {
                  ...message,
                  content: `Request failed: ${payload.message ?? 'Unexpected error'}`,
                  isStreaming: false,
                }
              : message,
          ),
        )
        setIsSending(false)
        currentRequestIdRef.current = null
      }
    }).then((dispose) => {
      if (disposed) {
        dispose()
      } else {
        unlisten = dispose
      }
    })

    return () => {
      disposed = true
      unlisten?.()
    }
  }, [])

  async function send() {
    const content = draft.trim()
    if (!content || isSending || !active || !model) return

    const userMessage: ChatItem = { id: crypto.randomUUID(), role: 'user', content }
    const conversation = [...messages.filter((item) => item.id !== 'welcome'), userMessage]
    const requestId = crypto.randomUUID()
    currentRequestIdRef.current = requestId
    setMessages([...conversation, { id: requestId, role: 'assistant', model, content: '', reasoningText: '', isStreaming: true, toolCalls: [] }])
    setDraft('')
    setIsSending(true)

    try {
      const response = await providersApi.chatGenerateStream({
        requestId,
        providerId: active.id,
        model,
        messages: conversation.map((item) => ({ role: item.role, content: item.content })),
      })
      setMessages((current) =>
        current.map((message) =>
          message.id === requestId
            ? {
                ...message,
                content: response.text || message.content || '(empty response)',
                reasoningText: response.reasoningText ?? message.reasoningText,
                isStreaming: false,
              }
            : message,
        ),
      )
    } catch (sendError) {
      setMessages((current) =>
        current.map((message) =>
          message.id === requestId
            ? { ...message, content: `Request failed: ${resolveMessage(sendError)}`, isStreaming: false }
            : message,
        ),
      )
    } finally {
      setIsSending(false)
      currentRequestIdRef.current = null
    }
  }

  return (
    <div className="grid h-full min-h-0 grid-rows-[minmax(0,1fr)_auto] bg-zinc-50">
      <div className="min-h-0 overflow-auto" aria-live="polite">
        {visibleMessages.length === 0 ? (
          <EmptySessionHero active={!!active} />
        ) : (
          <div className="mx-auto flex w-full max-w-5xl flex-col gap-4 px-6 py-8 max-[560px]:px-4">
            {visibleMessages.map((message) =>
              message.role === 'user' ? (
                <UserMessage key={message.id} content={message.content} />
              ) : (
                <AssistantMessage
                  key={message.id}
                  content={message.content}
                  reasoningText={message.reasoningText}
                  model={message.model}
                  isStreaming={message.isStreaming}
                  toolCalls={message.toolCalls}
                />
              ),
            )}
          </div>
        )}
      </div>

      <ChatInput
        activeProviderName={active?.name ?? 'No provider'}
        model={model}
        modelOptions={active?.models ?? []}
        value={draft}
        isSending={isSending}
        disabled={!active}
        onValueChange={setDraft}
        onModelChange={setModel}
        onSubmit={() => void send()}
      />
    </div>
  )
}

function EmptySessionHero({ active }: { active: boolean }) {
  return (
    <div className="grid min-h-full place-items-center px-6 py-14 text-center">
      <div className="mb-20 max-w-md">
        <div className="mx-auto mb-8 grid size-20 place-items-center text-orange-500">
          <div className="relative">
            <SquareTerminal size={62} strokeWidth={2.2} className="text-slate-900" />
            <Sparkles size={20} className="absolute -right-3 top-1 text-orange-400" />
          </div>
        </div>
        <h2 className="text-3xl font-semibold tracking-normal text-slate-950">新建会话</h2>
        <p className="mt-4 text-base leading-7 text-stone-600">
          {active
            ? '开始一个新的编码会话。Anvil 已准备好帮你构建、调试和梳理项目。'
            : '先在 Settings 中配置并启用一个云端 Provider，然后开始新的编码会话。'}
        </p>
      </div>
    </div>
  )
}

function resolveMessage(error: unknown): string {
  if (typeof error === 'string') return error
  if (error instanceof Error) return error.message
  return 'Unexpected error'
}
