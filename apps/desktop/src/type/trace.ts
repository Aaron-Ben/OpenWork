export type TraceSpanKind =
  | 'turn'
  | 'step'
  | 'model_attempt'
  | 'transport_attempt'
  | 'tool_run'
  | 'approval'
  | 'recovery'

export type TraceSpanStatus =
  | 'running'
  | 'waiting'
  | 'succeeded'
  | 'failed'
  | 'cancelled'
  | 'denied'
  | 'outcome_unknown'

export interface TraceSpan {
  traceId: string
  spanId: string
  parentSpanId: string | null
  spanKind: TraceSpanKind
  spanName: string
  status: TraceSpanStatus
  sessionId: string
  turnId: string
  stepId: string | null
  toolRunId: string | null
  startedAt: number
  endedAt: number | null
  durationMs: number | null
  attributes: Record<string, unknown>
  errorType: string | null
  errorCode: string | null
  errorMessage: string | null
}

export interface TurnTraceSummary {
  traceId: string
  turnId: string
  sessionId: string
  status: TraceSpanStatus
  model: string | null
  startedAt: number
  endedAt: number | null
  durationMs: number
  stepCount: number
  modelAttemptCount: number
  transportAttemptCount: number
  toolRunCount: number
  approvalCount: number
  retryCount: number
  inputTokens: number
  outputTokens: number
  errorCount: number
  recovered: boolean
}

export interface TurnTrace {
  summary: TurnTraceSummary
  spans: TraceSpan[]
}
