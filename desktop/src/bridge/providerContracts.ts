// Mirrors the camelCase serde output from openwork-providers. These are
// transport contracts shared by the workbench and collaboration shells.

export type ProviderKind =
  | 'openai'
  | 'glm'
  | 'kimi'
  | 'deepseek'
  | 'qwen'
  | 'anthropic'

export type ModelTier = 'lite' | 'plus' | 'pro'

export interface ModelCapabilities {
  contextWindowTokens: number
  maxOutputTokens: number
  maxReasoningTokens: number | null
  acceptsDataBlocks: boolean
}

export interface ProviderModel {
  modelId: string
  displayName?: string
  modelTier: ModelTier
  enabled: boolean
  capabilities?: ModelCapabilities
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
  models: Array<Pick<ProviderModel, 'modelId' | 'modelTier' | 'capabilities'>>
  websiteUrl: string
  apiKeyUrl: string
}

export interface ProviderIndex {
  providers: ProviderConfig[]
}

export interface TestResult {
  success: boolean
  message: string
}
