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

export type TraceDiagnosisStatus = 'healthy' | 'attention' | 'blocked'

export type TraceDiagnosisReason =
  | 'healthy'
  | 'waiting_approval'
  | 'model_error'
  | 'transport_exhausted'
  | 'tool_error'
  | 'approval_denied'
  | 'outcome_unknown'
  | 'cancelled'
  | 'recovered'
  | 'partial_trace'

export type TraceDataCompleteness = 'complete' | 'partial' | 'legacy' | 'unknown'

export interface TraceDiagnosis {
  status: TraceDiagnosisStatus
  reason: TraceDiagnosisReason
  focusSpanId: string | null
  evidenceSpanIds: string[]
}

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
  sessionTitle: string | null
  workingDir: string | null
  inputPreview: string | null
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
  diagnosis: TraceDiagnosis
  dataCompleteness: TraceDataCompleteness
}

export interface TurnTrace {
  summary: TurnTraceSummary
  spans: TraceSpan[]
}

export interface TraceListPage {
  items: TurnTraceSummary[]
  nextOffset: number | null
}

export interface TraceListQuery {
  limit?: number
  offset?: number
  query?: string
  sessionId?: string
  project?: string
  model?: string
  status?: TraceSpanStatus
  startedAfter?: number
  startedBefore?: number
  hasError?: boolean
  hasRetry?: boolean
}

export interface TraceSpanDetailView {
  span: TraceSpan
  detail: TraceSpanDetail
}

export interface TraceObservation {
  status: 'succeeded' | 'failed' | 'denied' | 'cancelled' | 'outcome_unknown'
  content: Array<{ type: string; text?: string }>
  error: { code: string; message: string; retryable: boolean } | null
}

export interface TraceTokenUsage {
  inputTokens: number
  outputTokens: number
  totalTokens: number
  cachedInputTokens: number
  reasoningTokens: number
}

export interface TraceModelRequestSummary {
  version: string | null
  messageCount: number | null
  messageTextChars: number | null
  systemPromptChars: number | null
  toolDefinitionCount: number | null
  toolNames: string[]
  temperature: number | null
  maxOutputTokens: number | null
  thinkingMode: string | null
}

export type TraceSpanDetail =
  | {
      kind: 'turn'
      data: {
        traceSchemaVersion: string | null
        instrumentationVersion: string | null
        appVersion: string | null
        captureMode: string | null
        outcome: string | null
        diagnosis: TraceDiagnosis
        dataCompleteness: TraceDataCompleteness
      }
    }
  | {
      kind: 'step'
      data: {
        stepIndex: number | null
        toolCount: number
        messages: import('./session').SessionMessage[]
      }
    }
  | {
      kind: 'model_attempt'
      data: {
        providerId: string | null
        model: string | null
        finishReason: string | null
        rawFinishReason: string | null
        responseId: string | null
        providerRequestId: string | null
        firstOutputMs: number | null
        usage: TraceTokenUsage
        requestSummary: TraceModelRequestSummary
        messages: import('./session').SessionMessage[]
      }
    }
  | {
      kind: 'transport_attempt'
      data: {
        providerId: string | null
        transportAttempt: number | null
        httpStatus: number | null
        providerCode: string | null
        providerRequestId: string | null
        retryDelayMs: number | null
        willRetry: boolean | null
        failurePhase: string | null
        deliveryState: string | null
      }
    }
  | {
      kind: 'tool_run'
      data: {
        toolName: string | null
        providerToolCallId: string | null
        input: unknown | null
        observation: TraceObservation | null
        approvalRequired: boolean
        requestedAt: number | null
        executionStartedAt: number | null
        requestToEndMs: number | null
        approvalWaitMs: number | null
        executionMs: number | null
      }
    }
  | {
      kind: 'approval'
      data: {
        reason: string | null
        toolName: string | null
        resolution: string | null
        waitMs: number | null
      }
    }
  | {
      kind: 'recovery'
      data: {
        reason: string | null
        approvalId: string | null
        stepIndex: number | null
      }
    }
