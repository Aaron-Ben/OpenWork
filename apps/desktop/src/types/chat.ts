import type { ContentBlock } from './parts'

/// 前端渲染单元:一条消息(user/assistant/tool)。`parts` 为有序 ContentBlock。
export interface ChatItem {
  id: string
  turnId?: string
  role: 'user' | 'assistant' | 'tool'
  parts: ContentBlock[]
  model?: string
  isStreaming?: boolean
  isCompacting?: boolean
  requestId?: string
  fileChangePresentation?: 'activity' | 'summary'
}
