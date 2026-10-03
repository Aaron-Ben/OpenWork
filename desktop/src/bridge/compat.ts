// Temporary hand-written mirror of the Rust Host Contract.
// Keep all compatibility DTOs in this bridge boundary until generated.ts lands.
import type { ContentBlock, ToolResultArtifact } from '@/types/parts'

export const RUNTIME_SESSION_UPDATE_VERSION = 7

export function supportsRuntimeSessionUpdateVersion(version: number): boolean {
  // V2 added tool progress, V3 added terminal tool artifacts, V4 added the
  // compacting phase, V5 added structured permission cards, V6 added
  // session-scoped approval actions, and V7 replaced them with sandbox modes,
  // escalation / dangerous-command cards and the sandbox status in snapshots.
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
  parentSessionId: string | null
  taskName: string | null
  agentRole: string | null
  spawnSpanId: string | null
  /** 会话持久化的沙箱模式（permissions.md §13.1）；运行中的模式以快照为准。 */
  sandboxMode: RuntimePermissionMode
}

export type RuntimeSubAgentSessionRecord = RuntimeSessionRecord & {
  parentSessionId: string
  taskName: string
  agentRole: string
}

export type RuntimeSkillSource = 'agents'

export interface RuntimeSkillSummary {
  source: RuntimeSkillSource
  name: string
  description: string
  path: string
  disabled: boolean
}

export interface RuntimeSkillWarning {
  path: string
  reason: string
}

export interface RuntimeSkillDiscovery {
  skills: RuntimeSkillSummary[]
  warnings: RuntimeSkillWarning[]
}

export interface RuntimeSkillDetail {
  source: RuntimeSkillSource
  name: string
  description: string
  path: string
  body: string
}

export type RuntimeUserInput =
  | { type: 'text'; text: string }
  | { type: 'skill'; name: string; path: string }

export type RuntimeSkillInput = Extract<RuntimeUserInput, { type: 'skill' }>

export interface RuntimeStoredMessage {
  id: string
  turnId: string | null
  sequence: number
  role: 'system' | 'user' | 'assistant' | 'tool'
  content: ContentBlock[]
  messageKind: 'normal' | 'skill_instruction' | 'agent_message' | 'world_state'
  createdAt: string
}

/** 与工具参数同一套 wire 形式，不再维护第二套状态映射。 */
export type RuntimePlanStepStatus = 'pending' | 'in_progress' | 'completed'

export interface RuntimePlanStep {
  step: string
  status: RuntimePlanStepStatus
}

/** 活动 Turn 的计划。承载它的 snapshot / envelope 已经有 turnId，这里不重复。 */
export interface RuntimeTurnPlanSnapshot {
  explanation: string | null
  steps: RuntimePlanStep[]
  updatedAt: string
}

/** 历史 Turn 的最终计划。 */
export interface RuntimeTurnPlan {
  turnId: string
  explanation: string | null
  steps: RuntimePlanStep[]
  updatedAt: string
}

export interface RuntimeLoadedSession {
  session: RuntimeSessionRecord
  messages: RuntimeStoredMessage[]
  /**
   * 按 Turn 返回的最终计划，列表而非 Record：Rust DTO 保持自然结构，
   * 前端在 transcript 投影边界一次性按 turnId 建索引。
   */
  plans: RuntimeTurnPlan[]
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
  contextWindowTokens: number
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

/** permissions.md §2：只有两个模式。 */
export type RuntimePermissionMode = 'auto' | 'accept_edits'
/** 卡片上只有两个按钮（permissions.md §12.2）。 */
export type RuntimePermissionDecision = 'allow_once' | 'deny'

/** 启动自检的结论（permissions.md §6）。 */
export type RuntimeSandboxStatus =
  | { state: 'available' }
  | { state: 'unavailable'; reason: string }

export type RuntimeGrantAccess = 'read' | 'write'
export type RuntimeGrantScope = 'exact' | 'subtree'
export type RuntimePathTier = 'hard_protected' | 'sensitive' | 'credential' | 'normal'
export type RuntimeDangerKey =
  | 'rm_recursive_or_force'
  | 'find_delete'
  | 'git_clean_force'
  | 'nesting_too_deep'

/** 越界请求的一条路径。 */
export interface RuntimeApprovalPath {
  path: string
  access: RuntimeGrantAccess
  scope: RuntimeGrantScope
  tier: RuntimePathTier
  inWorkspace: boolean
}

/** 命中的危险命令；`start` / `end` 是 `command` 里的 UTF-16 偏移。 */
export interface RuntimeApprovalDanger {
  key: RuntimeDangerKey
  start: number
  end: number
}

/** 越界与危险命令可以出现在同一张卡片上（permissions.md §12.1）。 */
export interface RuntimeApprovalCard {
  mode: RuntimePermissionMode
  command: string | null
  justification: string | null
  paths: RuntimeApprovalPath[]
  danger: RuntimeApprovalDanger | null
  previousDenial: string | null
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
  | {
      type: 'permission_resolved'
      toolCallId: string
      decision: RuntimePermissionDecision
    }
  | {
      /**
       * 每次都是完整快照，reducer 整体替换而不是 merge。
       * 丢一条不影响业务状态：snapshot 与持久层都能恢复。
       */
      type: 'plan_updated'
      explanation: string | null
      plan: RuntimePlanStep[]
      updatedAt: string
    }
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
      plan: RuntimeTurnPlanSnapshot | null
    }
  | {
      state: 'terminal'
      turnId: string
      clientRequestId: string
      outcome: RuntimeTurnOutcome
      /** Turn 刚结束时同进程重连不该丢掉计划卡。 */
      plan: RuntimeTurnPlanSnapshot | null
    }

export interface RuntimeSessionSnapshot {
  version: number
  sessionId: string
  lastUpdateSequence: number
  permissionMode: RuntimePermissionMode
  sandbox: RuntimeSandboxStatus
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
  /** 整条 Trace 的 token 合计（各 span 的 input + output 求和），跨 provider 不可直接比较。 */
  totalTokens: number
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
