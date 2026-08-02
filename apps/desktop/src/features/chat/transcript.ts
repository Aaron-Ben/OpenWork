import type { ChatItem } from '@/types/chat'
import type { ContentBlock, ToolResultState } from '@/types/parts'
import type { RuntimeLiveToolCall, RuntimeStoredMessage } from '@/bridge/compat'
import {
  appendCompletedFileChangeSummaries,
  mergeToolMessages,
} from './components/toolActivity'
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

function liveToolResultPart(toolCall: RuntimeLiveToolCall): ContentBlock | null {
  if (toolCall.output == null) return null
  return {
    type: 'tool_result',
    id: toolCall.providerCallId,
    name: toolCall.name,
    output: [{ type: 'text', text: toolCall.output }],
    state: resultState(toolCall),
    ...(toolCall.artifacts?.length ? { artifacts: toolCall.artifacts } : {}),
  }
}

function toolParts(
  runtime: SessionRuntimeView,
  representedToolCallIds: ReadonlySet<string>,
): ContentBlock[] {
  return runtime.orderedToolCallIds.flatMap((id) => {
    const toolCall = runtime.toolCalls[id]
    if (!toolCall || representedToolCallIds.has(toolCall.providerCallId)) return []
    const parts: ContentBlock[] = [{
      type: 'tool_call',
      id: toolCall.providerCallId,
      name: toolCall.name,
      input: safeStringify(toolCall.input),
      state: toolCall.isError == null ? 'submitted' : 'finished',
    }]
    const result = liveToolResultPart(toolCall)
    if (result) parts.push(result)
    return parts
  })
}

function attachLiveResultsToCanonicalCalls(
  transcript: ChatItem[],
  runtime: SessionRuntimeView,
  persistedToolResultIds: ReadonlySet<string>,
): ChatItem[] {
  const liveByProviderCallId = new Map(
    runtime.orderedToolCallIds.flatMap((id) => {
      const toolCall = runtime.toolCalls[id]
      return toolCall ? [[toolCall.providerCallId, toolCall] as const] : []
    }),
  )

  return transcript.map((message) => {
    if (message.role !== 'assistant' || message.turnId !== runtime.turnId) return message
    const liveResults = message.parts.flatMap((part) => {
      if (part.type !== 'tool_call' || persistedToolResultIds.has(part.id)) return []
      const toolCall = liveByProviderCallId.get(part.id)
      const result = toolCall ? liveToolResultPart(toolCall) : null
      return result ? [result] : []
    })
    return liveResults.length > 0
      ? { ...message, parts: [...message.parts, ...liveResults] }
      : message
  })
}

export function buildTranscript(
  messages: RuntimeStoredMessage[],
  runtime: SessionRuntimeView,
): ChatItem[] {
  let transcript = canonicalItems(messages)
  const persistedToolCallIds = new Set(
    messages.flatMap((message) =>
      message.turnId === runtime.turnId
        ? message.content.flatMap((part) => part.type === 'tool_call' ? [part.id] : [])
        : [],
    ),
  )
  const persistedToolResultIds = new Set(
    messages.flatMap((message) => message.content.flatMap((part) =>
      part.type === 'tool_result' ? [part.id] : [],
    )),
  )
  transcript = attachLiveResultsToCanonicalCalls(
    transcript,
    runtime,
    persistedToolResultIds,
  )
  const representedToolCallIds = new Set([
    ...persistedToolCallIds,
    ...persistedToolResultIds,
  ])
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
  parts.push(...toolParts(runtime, representedToolCallIds))
  const compacting = runtime.phase === 'compacting'
  if (runtime.turnId && (parts.length > 0 || compacting)) {
    transcript.push({
      id: `live-${runtime.turnId}`,
      turnId: runtime.turnId,
      role: 'assistant',
      parts,
      isStreaming: runtime.phase !== 'idle',
      isCompacting: compacting,
      requestId: runtime.clientRequestId ?? undefined,
    })
  }
  return appendCompletedFileChangeSummaries(
    mergeToolMessages(transcript),
    runtime.phase === 'idle' ? null : runtime.turnId,
  )
}
