import type { ChatItem } from '../../type/chat'
import type { ContentBlock, ToolResultState } from '../../type/parts'
import type { RuntimeLiveToolCall, RuntimeStoredMessage } from '../../bridge/compat'
import { mergeToolMessages } from './components/toolActivity'
import type { SessionRuntimeView } from './runtimeReducer'

function canonicalItems(messages: RuntimeStoredMessage[]): ChatItem[] {
  return messages
    .filter(
      (message): message is RuntimeStoredMessage & { role: 'user' | 'assistant' | 'tool' } =>
        message.role === 'user' || message.role === 'assistant' || message.role === 'tool',
    )
    .map((message) => ({
      id: message.id,
      turnId: message.turnId ?? undefined,
      role: message.role,
      parts: message.content,
    }))
}

function safeStringify(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2)
  } catch {
    return String(value)
  }
}

function resultState(toolCall: RuntimeLiveToolCall): ToolResultState {
  if (toolCall.isError == null) return 'running'
  if (toolCall.isError) return 'error'
  if (toolCall.status === 'denied') return 'denied'
  if (toolCall.status === 'cancelled' || toolCall.status === 'outcome_unknown') return 'interrupted'
  if (toolCall.output == null) return 'running'
  return 'success'
}

function toolParts(runtime: SessionRuntimeView): ContentBlock[] {
  return runtime.orderedToolCallIds.flatMap((id) => {
    const toolCall = runtime.toolCalls[id]
    if (!toolCall) return []
    const parts: ContentBlock[] = [{
      type: 'tool_call',
      id: toolCall.providerCallId,
      name: toolCall.name,
      input: safeStringify(toolCall.input),
      state: toolCall.isError == null ? 'submitted' : 'finished',
    }]
    if (toolCall.output != null) {
      parts.push({
        type: 'tool_result',
        id: toolCall.providerCallId,
        name: toolCall.name,
        output: [{ type: 'text', text: toolCall.output }],
        state: resultState(toolCall),
      })
    }
    return parts
  })
}

export function buildTranscript(
  messages: RuntimeStoredMessage[],
  runtime: SessionRuntimeView,
): ChatItem[] {
  const transcript = canonicalItems(messages)
  const canonicalTurnIds = new Set(
    messages.filter((message) => message.role === 'user').map((message) => message.turnId),
  )
  const pending = runtime.pendingUserMessage
  if (pending && (!pending.turnId || !canonicalTurnIds.has(pending.turnId))) {
    transcript.push({
      id: pending.id,
      turnId: pending.turnId ?? undefined,
      role: 'user',
      parts: [{ type: 'text', text: pending.text }],
    })
  }

  const parts: ContentBlock[] = []
  if (runtime.assistantDraft?.reasoning) {
    parts.push({ type: 'thinking', thinking: runtime.assistantDraft.reasoning })
  }
  if (runtime.assistantDraft?.text) {
    parts.push({ type: 'text', text: runtime.assistantDraft.text })
  }
  parts.push(...toolParts(runtime))
  if (runtime.turnId && parts.length > 0) {
    transcript.push({
      id: `live-${runtime.turnId}`,
      turnId: runtime.turnId,
      role: 'assistant',
      parts,
      isStreaming: runtime.phase !== 'idle',
      requestId: runtime.clientRequestId ?? undefined,
    })
  }
  return mergeToolMessages(transcript)
}
