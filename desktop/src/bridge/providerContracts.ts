// Mirrors the camelCase serde output of openwork-models `provider` types. Only the
// Responses wire protocol exists, so a provider is configuration data, not a kind.

export interface ModelCapabilities {
  contextWindowTokens: number
  maxOutputTokens: number
  maxReasoningTokens: number | null
  acceptsDataBlocks: boolean
}

export interface ProviderModel {
  modelId: string
  displayName?: string
  enabled: boolean
  capabilities?: ModelCapabilities
}

export interface ProviderSettings {
  name: string
  /** Responses base URL including the version path, e.g. https://api.moonshot.ai/v1 */
  baseUrl: string
  envKey?: string
  httpHeaders?: Record<string, string>
  queryParams?: Record<string, string>
  requestMaxRetries?: number
  streamIdleTimeoutMs?: number
  models: ProviderModel[]
  enabled: boolean
}

/** On update, an absent apiKey keeps the stored key. */
export interface ProviderInput extends ProviderSettings {
  apiKey?: string
}

/** Model metadata resolved from the bundled model catalog (openwork_models::catalog::ModelInfo). */
export interface ModelInfo {
  displayName: string
  capabilities: ModelCapabilities
  /** Responses `reasoning.effort` values, sent verbatim. Empty when the model does not reason. */
  reasoningEfforts: string[]
  defaultReasoningEffort: string | null
  source: 'configured' | 'catalog' | 'fallback'
}

export interface ResolvedModel extends ModelInfo {
  modelId: string
  enabled: boolean
}

export interface ProviderConfig extends ProviderSettings {
  id: string
  hasApiKey: boolean
  resolvedModels: ResolvedModel[]
}

export interface ProviderPreset {
  id: string
  name: string
  baseUrl: string
  models: Array<ModelInfo & { modelId: string }>
  websiteUrl: string
  apiKeyUrl: string
}

/** Model reference stored on a session: `<providerId>/<modelId>`. */
export function modelRef(providerId: string, modelId: string): string {
  return `${providerId}/${modelId}`
}

export interface ProviderIndex {
  providers: ProviderConfig[]
}

export interface TestResult {
  success: boolean
  message: string
}
