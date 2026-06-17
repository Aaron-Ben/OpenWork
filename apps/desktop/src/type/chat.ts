import type { ContentBlock } from './parts'

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
  approvalPolicy?: 'untrusted'
}

export interface ChatGenerateResponse {
  text: string
  reasoningText?: string | null
}
