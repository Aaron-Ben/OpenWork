import type { ChatItem, TurnPlanView } from '@/types/chat'
import type { ContentBlock, ToolResultState } from '@/types/parts'
import type {
  RuntimeLiveToolCall,
  RuntimeStoredMessage,
  RuntimeTurnPlan,
} from '@/bridge/compat'
import {
  appendCompletedFileChangeSummaries,
  mergeToolMessages,
} from './components/toolActivity'
import type { SessionRuntimeView } from './runtimeReducer'

function canonicalItems(messages: RuntimeStoredMessage[]): ChatItem[] {
  return messages
    .filter(
      (message): message is RuntimeStoredMessage & { role: 'user' | 'assistant' | 'tool' } =>
        message.messageKind === 'normal'
        && (message.role === 'user' || message.role === 'assistant' || message.role === 'tool'),
    )
    .map((message) => ({
      id: message.id,
      turnId: message.turnId ?? undefined,
      role: message.role,
      parts: message.content,
      createdAt: message.createdAt,
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

interface PlanCandidate {
  turnId: string
  explanation: string | null
  steps: TurnPlanView['steps']
  updatedAt: string
}

/**
 * 只投影会话中最新的计划。活动 Turn 的事件快照比加载时的持久化值新，
 * 但显式清空只应移除该 Turn 的旧值，不影响更早的已完成计划。
 */
function latestPlan(
  plans: RuntimeTurnPlan[],
  runtime: SessionRuntimeView,
): PlanCandidate | null {
  const candidates = plans
    .filter((plan) => plan.steps.length > 0 && plan.turnId !== runtime.turnId)
    .map((plan): PlanCandidate => plan)

  if (runtime.turnId && runtime.plan) {
    candidates.push({
      turnId: runtime.turnId,
      explanation: runtime.plan.explanation,
      steps: runtime.plan.steps,
      updatedAt: runtime.plan.updatedAt,
    })
  }

  return candidates.reduce<PlanCandidate | null>((latest, plan) => (
    !latest || Date.parse(plan.updatedAt) >= Date.parse(latest.updatedAt) ? plan : latest
  ), null)
}

function isPlanPart(part: ContentBlock): boolean {
  return (part.type === 'tool_call' || part.type === 'tool_result') && part.name === 'update_plan'
}

/** 隐藏计划工具块时仍保留源消息身份，否则 Trace 的“打开消息”会找不到被折叠的响应。 */
function stripPlanParts(transcript: ChatItem[]): ChatItem[] {
  const projected = transcript.map((item) => ({
    item: { ...item, parts: item.parts.filter((part) => !isPlanPart(part)) },
    hadNoParts: item.parts.length === 0,
    hiddenPlanOnly: item.parts.length > 0 && item.parts.every(isPlanPart),
  }))
  const hiddenIdsByTurn = new Map<string, string[]>()
  for (const entry of projected) {
    if (!entry.hiddenPlanOnly || !entry.item.turnId) continue
    const ids = entry.item.sourceMessageIds ?? [entry.item.id]
    hiddenIdsByTurn.set(entry.item.turnId, [
      ...(hiddenIdsByTurn.get(entry.item.turnId) ?? []),
      ...ids,
    ])
  }
  for (const [turnId, hiddenIds] of hiddenIdsByTurn) {
    const anchor = projected.find((entry) => entry.item.turnId === turnId && entry.item.plan)
      ?? projected.find((entry) => (
        entry.item.turnId === turnId
        && entry.item.role === 'assistant'
        && entry.item.parts.length > 0
      ))
      ?? projected.find((entry) => entry.item.turnId === turnId && entry.item.parts.length > 0)
    if (!anchor) continue
    anchor.item.sourceMessageIds = [...new Set([
      ...(anchor.item.sourceMessageIds ?? []),
      ...hiddenIds.filter((id) => id !== anchor.item.id),
    ])]
  }

  return projected.flatMap(({ item, hadNoParts }) => (
    item.parts.length > 0 || item.plan || hadNoParts ? [item] : []
  ))
}

/** update_plan 是状态更新：原始 Tool Call 从消息流消失，最新快照固定在首次调用处。 */
function projectPlan(transcript: ChatItem[], candidate: PlanCandidate | null): ChatItem[] {
  if (!candidate) return stripPlanParts(transcript)

  const calls = transcript.flatMap((item, itemIndex) => (
    item.turnId === candidate.turnId
      ? item.parts.flatMap((part) => part.type === 'tool_call' && part.name === 'update_plan'
        ? [{ id: part.id, itemIndex, createdAt: item.createdAt ?? null }]
        : [])
      : []
  ))
  const firstCall = calls[0]
  const fallbackIndex = transcript.findIndex(
    (item) => item.turnId === candidate.turnId && item.role === 'assistant',
  )
  const anchorIndex = firstCall?.itemIndex ?? fallbackIndex
  if (anchorIndex < 0) return transcript

  const plan: TurnPlanView = {
    explanation: candidate.explanation,
    steps: candidate.steps,
    updateCount: Math.max(1, new Set(calls.map((call) => call.id)).size),
    startedAt: firstCall?.createdAt ?? null,
    updatedAt: candidate.updatedAt,
  }

  return stripPlanParts(
    transcript.map((item, index) => ({
      ...item,
      ...(index === anchorIndex ? { plan } : {}),
    })),
  )
}

export function buildTranscript(
  messages: RuntimeStoredMessage[],
  runtime: SessionRuntimeView,
  plans: RuntimeTurnPlan[] = [],
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
  return projectPlan(
    appendCompletedFileChangeSummaries(
      mergeToolMessages(transcript),
      runtime.phase === 'idle' ? null : runtime.turnId,
    ),
    latestPlan(plans, runtime),
  )
}
