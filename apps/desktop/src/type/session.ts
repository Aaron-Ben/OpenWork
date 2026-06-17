// 镜像 anvil-session 的 serde(camelCase)输出。

import type { ContentBlock } from './parts'

export interface SessionSummary {
  id: string
  title: string
  providerId: string
  model: string
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

export interface SessionLoadResult {
  session: Session
  messages: SessionMessage[]
}

export interface WorktreeSnapshotSummary {
  id: string
  sessionId: string
  requestId: string
  workingDir: string
  changedFiles: string[]
  createdAt: number
  completedAt: number
  revertedAt: number | null
}

export interface WorktreeSnapshotFileDetail {
  path: string
  beforeText: string | null
  afterText: string | null
  binary: boolean
}

export interface WorktreeSnapshotDetail {
  snapshot: WorktreeSnapshotSummary
  files: WorktreeSnapshotFileDetail[]
}

export interface RevertSnapshotResponse {
  snapshot: WorktreeSnapshotSummary
}
