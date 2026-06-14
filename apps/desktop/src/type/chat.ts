import type { ToolCallState } from '../components/chat/ToolCallBlock'

export interface ChatItem {
  id: string
  role: 'user' | 'assistant'
  content: string
  reasoningText?: string | null
  model?: string
  isStreaming?: boolean
  toolCalls?: ToolCallState[]
}
