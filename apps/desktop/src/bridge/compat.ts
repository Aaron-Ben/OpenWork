// Temporary hand-written mirror of the Rust Host Contract.
// Keep all compatibility DTOs in this bridge boundary until generated.ts lands.
import type { ContentBlock, ToolResultArtifact } from '../type/parts'

export const RUNTIME_SESSION_UPDATE_VERSION = 5

export function supportsRuntimeSessionUpdateVersion(version: number): boolean {
  // V2 added tool progress, V3 added terminal tool artifacts, V4 added the
  // compacting phase, and V5 added structured permission cards. Older
  // versions remain readable during an
  // in-process rolling transition.
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
  autoCompactionThresholdPercent: number
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
  replacedThroughMessageSequence: number
  sourceMessageCount: number
  checkpointFormatVersion: number
  kind: 'manual' | 'threshold' | 'overflow' | 'rewind'
  summaryFormatVersion: number
  lastUserMessageId: string | null
  lastUserMessageSequence: number | null
  resolvedModelName: string
  summary: string
  runtimeState: RuntimeCompactionState
  runtimeReminderFormatVersion: number
  runtimeReminder: string
  triggerTurnId: string | null
  parentCompactionId: string | null
  inputTokens: number | null
  outputTokens: number | null
  createdAt: string
}

export interface RuntimeCompactionStateEntry {
  schemaVersion: number
  value: unknown
}

export interface RuntimeCompactionStateWarning {
  contributorKey: string
  code: string
}

export interface RuntimeCompactionState {
  schemaVersion: number
  editedPaths: string[]
  extensions: Record<string, RuntimeCompactionStateEntry>
  warnings: RuntimeCompactionStateWarning[]
}

export type RuntimeConversationProjectionSelector =
  | { type: 'latest' }
  | { type: 'compaction'; compactionId: string }
  | { type: 'through_message'; sequence: number }

export interface RuntimeConversationProjection {
  selector: RuntimeConversationProjectionSelector
  checkpointId: string | null
  throughMessageSequence: number
  messages: RuntimeStoredMessage[]
}

export interface RuntimeConversationTranscriptQuery {
  compactionId?: string | null
  afterSequence?: number | null
  limit?: number | null
}

export interface RuntimeConversationTranscriptPage {
  sessionId: string
  compactionId: string
  throughMessageSequence: number
  messages: RuntimeStoredMessage[]
  nextAfterSequence: number | null
  hasMore: boolean
}

export interface RuntimeTurnAccepted {
  turnId: string
  clientRequestId: string
}

export type RuntimePermissionMode = 'default' | 'accept_edits'

export type RuntimePermissionEffect =
  | { kind: 'read'; path: string }
  | { kind: 'write'; path: string }
  | { kind: 'exec'; program: string; args: string[] }

export type RuntimeEffectDisplay =
  | { certainty: 'inferred'; effect: RuntimePermissionEffect }
  | { certainty: 'readonly_proof'; key: string }
  | { certainty: 'trusted_program'; program: string }

export type RuntimeUnitVerdict =
  | { decision: 'allow'; source: 'builtin' | 'mode' | 'readonly_proof'; ruleId: string | null }
  | {
      decision: 'ask'
      source: 'explicit_rule' | 'builtin_sensitive' | 'no_rule_covers' | 'unparsed'
      ruleId: string | null
    }
  | { decision: 'deny'; ruleId: string; silent: boolean }

export interface RuntimePermissionCardUnit {
  display: string
  effects: RuntimeEffectDisplay[]
  verdict: RuntimeUnitVerdict
  outsideWorkspace: boolean
}

export interface RuntimeApprovalCard {
  units: RuntimePermissionCardUnit[]
  raw: string
  unparsed: boolean
}

export interface RuntimePermissionRequest {
  sessionId: string
  turnId: string
  toolCallId: string
  providerCallId: string
  toolName: string
  card: RuntimeApprovalCard
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
  | { type: 'phase_changed'; phase: 'starting' | 'running_model' | 'running_tools' | 'waiting_permission' | 'compacting' }
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
      phase: 'starting' | 'running_model' | 'running_tools' | 'waiting_permission' | 'compacting'
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
  permissionMode: RuntimePermissionMode
  runtime: RuntimeSnapshotState
}

export interface RuntimeTraceSummary {
  traceId: string
  /** 手动压缩与 rewind 没有 Turn，这两个字段为空，调用计数为 0。 */
  turnId: string | null
  sessionId: string
  turnSequence: number | null
  status: string
  resolvedModelName: string
  modelCallCount: number
  modelSubmissionCount: number
  toolCallCount: number
  spanCount: number
  startedAt: string
  endedAt: string | null
}

export interface RuntimeTraceSpan {
  id: string
  traceId: string
  sessionId: string
  turnId: string | null
  parentSpanId: string | null
  kind: 'model_call' | 'tool_call' | 'compaction'
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
  responseMessageId: string | null
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

export type RuntimeTracePayloadSlot =
  | 'request'
  | 'system_context'
  | 'tool_definitions'
  | 'response'

export type RuntimeTraceContentPolicy = 'full' | 'compaction_only' | 'off'

export interface RuntimeTraceSpanPayload {
  spanId: string
  slot: RuntimeTracePayloadSlot
  body: unknown
  byteSize: number
  truncated: boolean
  originalByteSize: number | null
  /** Retained only for the hand-written host contract; the UI intentionally does not display it. */
  redactedCount: number
}
