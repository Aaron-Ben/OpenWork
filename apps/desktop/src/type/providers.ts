// 镜像 anvil-providers 的 serde 输出(camelCase)。仅云端 API-key provider —— 无本地模型、无登录。

export type ProviderKind =
  | 'openai'
  | 'glm'
  | 'kimi'
  | 'deepseek'
  | 'qwen'
  | 'anthropic'
  | 'openai_compatible'

export interface ProviderInput {
  name: string
  baseUrl: string
  apiKey: string
  kind: ProviderKind
  models: string[]
  enabled: boolean
  extraBody?: Record<string, unknown>
}

export interface ProviderConfig extends ProviderInput {
  id: string
}

export interface ProviderPreset {
  id: string
  name: string
  baseUrl: string
  kind: ProviderKind
  models: string[]
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

export interface ChatMessage {
  role: 'user' | 'assistant' | 'system'
  content: string
}

export interface ChatGenerateRequest {
  providerId: string
  model: string
  messages: ChatMessage[]
}

export interface ChatGenerateStreamRequest extends ChatGenerateRequest {
  requestId: string
}

export interface ChatGenerateResponse {
  text: string
  reasoningText?: string | null
}

export type ChatStreamEventName = 'text_delta' | 'reasoning_delta' | 'done' | 'error'

export interface ChatStreamEventPayload {
  requestId: string
  event: ChatStreamEventName
  delta?: string | null
  message?: string | null
}
