import type { ContentBlock } from './parts'
import type { RuntimePlanStep } from '@/bridge/compat'

/// 会话中最新的计划快照。步骤顺序仍以服务端为准。
export interface TurnPlanView {
  explanation: string | null
  steps: RuntimePlanStep[]
  updateCount: number
  startedAt: string | null
  updatedAt: string
}

/// 前端渲染单元:一条消息(user/assistant/tool)。`parts` 为有序 ContentBlock。
export interface ChatItem {
  id: string
  /// 相邻只读工具消息合并展示时保留的全部源消息 ID，供 Trace 跳转与高亮定位。
  sourceMessageIds?: string[]
  turnId?: string
  role: 'user' | 'assistant' | 'tool'
  parts: ContentBlock[]
  createdAt?: string
  model?: string
  isStreaming?: boolean
  /// 该消息所属 Turn 仍在执行；文件变更只有在 Turn 结束后才能撤销。
  turnActive?: boolean
  isCompacting?: boolean
  requestId?: string
  fileChangePresentation?: 'activity' | 'summary'
  /// 会话中最新的计划，固定在它第一次 update_plan 的位置。
  plan?: TurnPlanView
}
