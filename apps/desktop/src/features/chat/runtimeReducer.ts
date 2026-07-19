import type {
  RuntimeLiveToolCall,
  RuntimePermissionRequest,
  RuntimeSessionSnapshot,
  RuntimeSessionUpdateEnvelope,
  RuntimeToolProgress,
  RuntimeTurnOutcome,
} from '../../bridge/compat'
import { supportsRuntimeSessionUpdateVersion } from '../../bridge/compat'

const MAX_LIVE_TOOL_OUTPUT_CHARS = 32 * 1024

export type SessionRuntimePhase =
  | 'idle'
  | 'starting'
  | 'running_model'
  | 'running_tools'
  | 'waiting_permission'

export interface PendingUserMessage {
  id: string
  clientRequestId: string
  turnId: string | null
  text: string
  state: 'pending' | 'failed'
  error: string | null
}

export interface AssistantDraft {
  turnId: string
  text: string
  reasoning: string
}

export interface SessionRuntimeView {
  lastSequence: number
  turnId: string | null
  clientRequestId: string | null
  phase: SessionRuntimePhase
  pendingUserMessage: PendingUserMessage | null
  assistantDraft: AssistantDraft | null
  toolCalls: Record<string, RuntimeLiveToolCall>
  orderedToolCallIds: string[]
  pendingPermission: RuntimePermissionRequest | null
  terminal: RuntimeTurnOutcome | null
  syncState: 'current' | 'stale' | 'resyncing'
  error: string | null
}

export function createSessionRuntimeView(): SessionRuntimeView {
  return {
    lastSequence: 0,
    turnId: null,
    clientRequestId: null,
    phase: 'idle',
    pendingUserMessage: null,
    assistantDraft: null,
    toolCalls: {},
    orderedToolCallIds: [],
    pendingPermission: null,
    terminal: null,
    syncState: 'current',
    error: null,
  }
}

function draftFor(state: SessionRuntimeView, turnId: string): AssistantDraft {
  return state.assistantDraft?.turnId === turnId
    ? state.assistantDraft
    : { turnId, text: '', reasoning: '' }
}

function appendToolProgress(
  output: string | null,
  progress: RuntimeToolProgress,
): string {
  const current = output ?? ''
  let addition: string
  switch (progress.kind) {
    case 'stdout':
      addition = progress.chunk
      break
    case 'stderr':
      addition = `${current && !current.endsWith('\n') ? '\n' : ''}[stderr]\n${progress.chunk}`
      break
    case 'message':
      addition = `${current && !current.endsWith('\n') ? '\n' : ''}[progress] ${progress.message}\n`
      break
  }
  return (current + addition).slice(-MAX_LIVE_TOOL_OUTPUT_CHARS)
}

export function reduceSessionUpdate(
  state: SessionRuntimeView,
  envelope: RuntimeSessionUpdateEnvelope,
): SessionRuntimeView {
  if (!supportsRuntimeSessionUpdateVersion(envelope.version)) {
    return {
      ...state,
      syncState: 'stale',
      error: `Unsupported session update version: ${envelope.version}`,
    }
  }
  if (envelope.sequence <= state.lastSequence) return state
  if (envelope.sequence > state.lastSequence + 1) {
    return { ...state, syncState: 'stale' }
  }

  const { update, turnId } = envelope
  const next: SessionRuntimeView = {
    ...state,
    lastSequence: envelope.sequence,
    turnId,
    syncState: 'current',
    error: null,
  }

  switch (update.type) {
    case 'turn_started':
      return {
        ...next,
        clientRequestId: update.clientRequestId,
        phase: state.phase === 'idle' ? 'starting' : state.phase,
        terminal: null,
        pendingUserMessage:
          state.pendingUserMessage?.clientRequestId === update.clientRequestId
            ? { ...state.pendingUserMessage, turnId }
            : state.pendingUserMessage,
      }
    case 'phase_changed':
      return { ...next, phase: update.phase }
    case 'text_delta': {
      const draft = draftFor(state, turnId)
      return { ...next, assistantDraft: { ...draft, text: draft.text + update.delta } }
    }
    case 'reasoning_delta': {
      const draft = draftFor(state, turnId)
      return { ...next, assistantDraft: { ...draft, reasoning: draft.reasoning + update.delta } }
    }
    case 'draft_cleared':
      return { ...next, assistantDraft: null }
    case 'tool_call_started': {
      const id = update.toolCall.toolCallId
      return {
        ...next,
        toolCalls: { ...state.toolCalls, [id]: update.toolCall },
        orderedToolCallIds: state.orderedToolCallIds.includes(id)
          ? state.orderedToolCallIds
          : [...state.orderedToolCallIds, id],
      }
    }
    case 'tool_call_progress': {
      const previous = state.toolCalls[update.toolCallId]
      if (!previous) return next
      return {
        ...next,
        toolCalls: {
          ...state.toolCalls,
          [update.toolCallId]: {
            ...previous,
            output: appendToolProgress(previous.output, update.progress),
          },
        },
      }
    }
    case 'tool_call_finished': {
      const previous = state.toolCalls[update.toolCallId]
      if (!previous) return next
      return {
        ...next,
        toolCalls: {
          ...state.toolCalls,
          [update.toolCallId]: {
            ...previous,
            providerCallId: update.providerCallId,
            name: update.toolName,
            status: update.status,
            output: update.output,
            isError: update.isError,
          },
        },
      }
    }
    case 'permission_requested':
      return {
        ...next,
        phase: 'waiting_permission',
        pendingPermission: update.request,
      }
    case 'permission_resolved':
      return {
        ...next,
        phase: 'running_tools',
        pendingPermission:
          state.pendingPermission?.toolCallId === update.toolCallId
            ? null
            : state.pendingPermission,
      }
    case 'turn_finished':
      return {
        ...next,
        phase: 'idle',
        pendingPermission: null,
        terminal: update.outcome,
      }
  }
}

export function runtimeViewFromSnapshot(snapshot: RuntimeSessionSnapshot): SessionRuntimeView {
  const base = {
    ...createSessionRuntimeView(),
    lastSequence: snapshot.lastUpdateSequence,
    syncState: 'current' as const,
  }
  const { runtime } = snapshot
  if (runtime.state === 'idle') return base
  if (runtime.state === 'terminal') {
    return {
      ...base,
      turnId: runtime.turnId,
      clientRequestId: runtime.clientRequestId,
      terminal: runtime.outcome,
    }
  }

  const toolCalls = Object.fromEntries(
    runtime.toolCalls.map((toolCall) => [toolCall.toolCallId, toolCall]),
  )
  return {
    ...base,
    turnId: runtime.turnId,
    clientRequestId: runtime.clientRequestId,
    phase: runtime.phase,
    assistantDraft: {
      turnId: runtime.turnId,
      text: runtime.draftText,
      reasoning: runtime.draftReasoning,
    },
    toolCalls,
    orderedToolCallIds: runtime.toolCalls.map((toolCall) => toolCall.toolCallId),
    pendingPermission: runtime.pendingPermission,
  }
}
