import type { ContentBlock } from './parts'
import type { RuntimePlanStep } from '@/bridge/compat'

/// 一个 Turn 的任务清单快照。位置即身份:服务端顺序原样保留。
export interface TurnPlanView {
  explanation: string | null
  steps: RuntimePlanStep[]
}

/// 前端渲染单元:一条消息(user/assistant/tool)。`parts` 为有序 ContentBlock。
export interface ChatItem {
  id: string
  /// 相邻只读工具消息合并展示时保留的全部源消息 ID，供 Trace 跳转与高亮定位。
  sourceMessageIds?: string[]
  turnId?: string
  role: 'user' | 'assistant' | 'tool'
  parts: ContentBlock[]
  model?: string
  isStreaming?: boolean
  isCompacting?: boolean
  requestId?: string
  fileChangePresentation?: 'activity' | 'summary'
  /// 该 Turn 的计划,只挂在这个 Turn 最后一条 assistant 消息上。
  plan?: TurnPlanView
}
