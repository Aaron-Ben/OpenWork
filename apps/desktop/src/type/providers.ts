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

interface TurnLiveEventBase {
  requestId: string
  sessionId: string
}

type LiveEvent<E extends string, P extends object = Record<never, never>> = TurnLiveEventBase &
  { event: E } & P

/// 镜像 `openwork_app::TurnLiveEventKind` 的 tagged union。
/// 每个事件只能携带该变体需要的字段，新增事件会迫使 reducer 穷尽处理。
export type TurnLiveEvent =
  | LiveEvent<'step', { step: number; stepId: string }>
  | LiveEvent<'llm_step_start', { step: number }>
  | LiveEvent<'llm_step_finish', { step: number; reason: string }>
  | LiveEvent<'llm_finish', { reason: string }>
  | LiveEvent<'text_start', { blockId: string }>
  | LiveEvent<'text_delta', { delta: string }>
  | LiveEvent<'text_end', { blockId: string }>
  | LiveEvent<'reasoning_start', { blockId: string }>
  | LiveEvent<'reasoning_delta', { delta: string }>
  | LiveEvent<'reasoning_end', { blockId: string }>
  | LiveEvent<'tool_call_start', { toolCallId: string; toolName: string }>
  | LiveEvent<'tool_call_delta', { toolCallId: string; partialInput: string }>
  | LiveEvent<'tool_call_end', { toolCallId: string }>
  | LiveEvent<
      'tool_result',
      {
        toolCallId: string
        toolRunId: string
        toolName: string
        output: string
        isError: boolean
      }
    >
  | LiveEvent<
      'approval_request',
      {
        approvalId: string
        toolRunId: string
        toolName: string
        input: unknown
        reason: string
      }
    >
  | LiveEvent<'approval_resolved', { approvalId: string }>
  | LiveEvent<'finished', { text: string }>
  | LiveEvent<'done'>
  | LiveEvent<'cancelled'>
  | LiveEvent<'doom_loop', { toolName: string }>
  | LiveEvent<'error', { message: string }>
