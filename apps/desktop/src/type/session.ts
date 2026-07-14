// 镜像 openwork-persistence::session 的 serde(camelCase) 输出。

import type { ContentBlock } from './parts'

export interface SessionSummary {
  id: string
  title: string
  providerId: string
  model: string
  workingDir: string | null
  updatedAt: number
}

export interface Session extends SessionSummary {
  workingDir: string | null
  createdAt: number
}

export interface SessionMessage {
  id: string
  sessionId: string
  role: 'system' | 'user' | 'assistant' | 'tool'
  parts: ContentBlock[]
  seq: number
  createdAt: number
}

export interface SessionInput {
  title?: string
  providerId: string
  model: string
  workingDir?: string | null
}

export type TurnLifecycleStatus =
  | 'running'
  | 'waiting_approval'
  | 'outcome_unknown'
  | 'interrupted'
  | 'completed'
  | 'cancelled'
  | 'doom_loop'
  | 'failed'

export type StepLifecycleStatus =
  | 'running'
  | 'waiting_approval'
  | 'outcome_unknown'
  | 'completed'
  | 'failed'

export type ToolRunLifecycleStatus =
  | 'requested'
  | 'waiting_approval'
  | 'running'
  | 'succeeded'
  | 'failed'
  | 'denied'
  | 'cancelled'
  | 'outcome_unknown'

export interface PendingApprovalSnapshot {
  approvalId: string
  turnId: string
  stepId: string
  stepIndex: number
  toolRunId: string
  providerToolCallId: string
  toolName: string
  input: unknown
  reason: string
}

export interface ToolRunLifecycleSnapshot {
  id: string
  providerToolCallId: string
  toolName: string
  input: unknown
  status: ToolRunLifecycleStatus
  observation: unknown | null
}

export interface StepLifecycleSnapshot {
  id: string
  index: number
  status: StepLifecycleStatus
  toolRuns: ToolRunLifecycleSnapshot[]
}

export interface TurnLifecycleSnapshot {
  id: string
  sessionId: string
  providerId: string
  model: string
  status: TurnLifecycleStatus
  steps: StepLifecycleSnapshot[]
  pendingApproval: PendingApprovalSnapshot | null
  startedAt: number
  updatedAt: number
}

export interface SessionLoadResult {
  session: Session
  messages: SessionMessage[]
  turns: TurnLifecycleSnapshot[]
}
