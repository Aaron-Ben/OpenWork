// Temporary hand-written mirror of the Rust Host Contract.
// Keep all compatibility DTOs in this bridge boundary until generated.ts lands.
import type { ContentBlock, ToolResultArtifact } from '../type/parts'

export const RUNTIME_SESSION_UPDATE_VERSION = 3

export function supportsRuntimeSessionUpdateVersion(version: number): boolean {
  // V2 added tool progress and V3 added terminal tool artifacts. Older
  // versions remain readable during an in-process rolling transition.
  return version >= 1 && version <= RUNTIME_SESSION_UPDATE_VERSION
}

export interface RuntimeSessionInput {
  id: string
  title?: string | null
  workingDirectory: string
  defaultModelId?: string | null
}

export interface RuntimeSessionRecord {
  id: string
  title: string | null
  workingDirectory: string
  defaultModelId: string | null
  status: 'active' | 'archived'
  createdAt: string
  updatedAt: string
  lastTurnAt: string | null
}

export interface RuntimeStoredMessage {
  id: string
  turnId: string | null
  sequence: number
  role: 'system' | 'user' | 'assistant' | 'tool'
  content: ContentBlock[]
  createdAt: string
}

export interface RuntimeLoadedSession {
  session: RuntimeSessionRecord
  messages: RuntimeStoredMessage[]
}

export interface RuntimeContextInspectionSystemPart {
  sourceKey: string
  content: ContentBlock[]
}

export interface RuntimeContextInspectionMessage {
  messageId: string
  turnId: string | null
  role: 'system' | 'user' | 'assistant' | 'tool'
  content: ContentBlock[]
}

export interface RuntimeToolDefinition {
  name: string
  description: string
  parameters: unknown
}

export interface RuntimeContextInspectionBudget {
  systemContextTokens: number
  conversationTokens: number
  toolSurfaceTokens: number
  estimatedInputTokens: number
  reservedOutputTokens: number | null
}

export interface RuntimeContextWindowInspection {
  schemaVersion: number
  sessionId: string
  currentTurnId: string | null
  resolvedModelName: string
  systemContext: RuntimeContextInspectionSystemPart[]
  conversation: RuntimeContextInspectionMessage[]
  toolSurface: RuntimeToolDefinition[]
  budget: RuntimeContextInspectionBudget
}

export interface RuntimeConversationCompaction {
  id: string
  sessionId: string
  sequence: number
  throughMessageSequence: number
  sourceMessageCount: number
  resolvedModelName: string
  summary: string
  inputTokens: number | null
  outputTokens: number | null
  createdAt: string
}

export interface RuntimeTurnAccepted {
  turnId: string
  clientRequestId: string
}

export interface RuntimePermissionRequest {
  sessionId: string
  turnId: string
  toolCallId: string
  providerCallId: string
  toolName: string
  input: unknown
  reason: string
}

export interface RuntimeLiveToolCall {
  toolCallId: string
  providerCallId: string
  name: string
  input: unknown
  status: string
  output: string | null
  isError: boolean | null
  artifacts?: ToolResultArtifact[]
}

export type RuntimeToolProgress =
  | { kind: 'stdout'; chunk: string }
  | { kind: 'stderr'; chunk: string }
  | { kind: 'message'; message: string }

export type RuntimeTurnOutcome =
  | { status: 'completed'; finalText: string }
  | { status: 'failed'; code: string; message: string }
  | { status: 'cancelled' }

export type RuntimeSessionUpdate =
  | { type: 'turn_started'; clientRequestId: string }
  | { type: 'phase_changed'; phase: 'starting' | 'running_model' | 'running_tools' | 'waiting_permission' }
  | { type: 'text_delta'; delta: string }
  | { type: 'reasoning_delta'; delta: string }
  | { type: 'draft_cleared' }
  | { type: 'tool_call_started'; toolCall: RuntimeLiveToolCall }
  | { type: 'tool_call_progress'; toolCallId: string; progress: RuntimeToolProgress }
  | {
      type: 'tool_call_finished'
      toolCallId: string
      providerCallId: string
      toolName: string
      status: string
      output: string
      isError: boolean
      artifacts?: ToolResultArtifact[]
    }
  | { type: 'permission_requested'; request: RuntimePermissionRequest }
  | { type: 'permission_resolved'; toolCallId: string; decision: 'allow' | 'deny' }
  | { type: 'turn_finished'; outcome: RuntimeTurnOutcome }

export interface RuntimeSessionUpdateEnvelope {
  version: number
  sessionId: string
  turnId: string
  sequence: number
  occurredAtMs: number
  update: RuntimeSessionUpdate
}

export interface RuntimeUndoFileChangesResult {
  undoneChangeIds: string[]
}

export interface RuntimeReapplyFileChangesResult {
  reappliedChangeIds: string[]
}

export type RuntimeSnapshotState =
  | { state: 'idle' }
  | {
      state: 'running'
      turnId: string
      clientRequestId: string
      phase: 'starting' | 'running_model' | 'running_tools' | 'waiting_permission'
      draftText: string
      draftReasoning: string
      toolCalls: RuntimeLiveToolCall[]
      pendingPermission: RuntimePermissionRequest | null
    }
  | {
      state: 'terminal'
      turnId: string
      clientRequestId: string
      outcome: RuntimeTurnOutcome
    }

export interface RuntimeSessionSnapshot {
  version: number
  sessionId: string
  lastUpdateSequence: number
  runtime: RuntimeSnapshotState
}

export interface RuntimeTraceSummary {
  turnId: string
  sessionId: string
  turnSequence: number
  status: string
  resolvedModelName: string
  modelCallCount: number
  toolCallCount: number
  spanCount: number
  startedAt: string
  endedAt: string | null
}

export interface RuntimeTraceSpan {
  id: string
  turnId: string
  parentSpanId: string | null
  sequence: number
  kind: 'model_call' | 'tool_call'
  name: string
  status: string
  modelId: string | null
  resolvedModelName: string | null
  providerRequestId: string | null
  providerCallId: string | null
  requestedToolName: string | null
  resolvedToolName: string | null
  attemptCount: number | null
  inputTokens: number | null
  outputTokens: number | null
  cachedInputTokens: number | null
  reasoningTokens: number | null
  totalTokens: number | null
  permissionWaitMs: number | null
  startedAt: string
  endedAt: string | null
  errorCode: string | null
  errorMessage: string | null
  attributes: Record<string, unknown>
}

export interface RuntimeTraceCompleteness {
  expectedModelCalls: number
  capturedModelCalls: number
  expectedToolCalls: number
  capturedToolCalls: number
  orphanToolSpans: number
  runningSpans: number
  outcomeUnknownSpans: number
  state: 'complete' | 'partial' | 'none'
}

export interface RuntimeTurnTrace {
  summary: RuntimeTraceSummary
  spans: RuntimeTraceSpan[]
  completeness: RuntimeTraceCompleteness
}
