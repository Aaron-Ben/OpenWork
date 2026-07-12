import type { ContentBlock } from './parts'

export const DEFAULT_APPROVAL_POLICY = 'untrusted' as const
export type ApprovalPolicy = typeof DEFAULT_APPROVAL_POLICY

/// 前端渲染单元:一条消息(user/assistant/tool)。`parts` 为有序 ContentBlock。
export interface ChatItem {
  id: string
  role: 'user' | 'assistant' | 'tool'
  parts: ContentBlock[]
  model?: string
  isStreaming?: boolean
  requestId?: string
}

export interface ChatGenerateStreamRequest {
  requestId: string
  sessionId: string
  providerId: string
  model: string
  userText: string
  approvalPolicy?: ApprovalPolicy
}

export interface ChatGenerateResponse {
  text: string
  reasoningText?: string | null
}
