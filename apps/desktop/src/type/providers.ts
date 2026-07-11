// 镜像 openwork-providers 的 serde 输出(camelCase)。仅云端 API-key provider —— 无本地模型、无登录。

export type ProviderKind =
  | 'openai'
  | 'glm'
  | 'kimi'
  | 'deepseek'
  | 'qwen'
  | 'anthropic'

export type ModelTier = 'lite' | 'plus' | 'pro'

export interface ProviderModel {
  modelId: string
  displayName?: string
  modelTier: ModelTier
  enabled: boolean
}

export interface ProviderInput {
  name: string
  baseUrl: string
  apiKey: string
  kind: ProviderKind
  models: ProviderModel[]
  enabled: boolean
  extraBody?: Record<string, unknown>
}

export interface ProviderConfig {
  id: string
  name: string
  baseUrl: string
  kind: ProviderKind
  models: ProviderModel[]
  enabled: boolean
}

export interface ProviderPreset {
  id: string
  name: string
  baseUrl: string
  kind: ProviderKind
  models: Array<Pick<ProviderModel, 'modelId' | 'modelTier'>>
  websiteUrl: string
  apiKeyUrl: string
}

export interface ProviderIndex {
  providers: ProviderConfig[]
  activeId: string | null
}

export interface TestResult {
  success: boolean
  message: string
}

export type ChatStreamEventName =
  | 'llm_step_start'
  | 'llm_step_finish'
  | 'llm_finish'
  | 'text_start'
  | 'text_delta'
  | 'text_end'
  | 'reasoning_start'
  | 'reasoning_delta'
  | 'reasoning_end'
  | 'step'
  | 'tool_call_start'
  | 'tool_call_delta'
  | 'tool_call_end'
  | 'tool_result'
  | 'approval_request'
  | 'approval_resolved'
  | 'finished'
  | 'done'
  | 'cancelled'
  | 'doom_loop'
  | 'error'

/// 前端 `chat-stream-event` 监听的单帧 payload。`sessionId` 用于多会话隔离分派。
export interface ChatStreamEventPayload {
  requestId: string
  sessionId: string
  event: ChatStreamEventName
  delta?: string | null
  message?: string | null
  step?: number | null
  toolCallId?: string | null
  toolName?: string | null
  partialInput?: string | null
  toolOutput?: string | null
  isError?: boolean | null
  approvalId?: string | null
  input?: unknown | null
}
