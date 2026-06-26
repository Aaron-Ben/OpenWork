// 镜像 openwork-session 的 serde(camelCase)输出。

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
