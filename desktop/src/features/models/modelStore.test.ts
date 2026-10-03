import { describe, expect, it } from 'vitest'

import type { ProviderConfig } from '@/bridge/providerContracts'
import { selectDefaultModel } from './modelStore'

const providers: ProviderConfig[] = [
  {
    id: 'disabled-provider',
    name: 'Disabled',
    baseUrl: 'https://disabled.invalid',
    enabled: false,
    hasApiKey: true,
    resolvedModels: [],
    models: [{ modelId: 'disabled-model', enabled: true }],
  },
  {
    id: 'ready-provider',
    name: 'Ready',
    baseUrl: 'https://ready.invalid',
    enabled: true,
    hasApiKey: true,
    resolvedModels: [],
    models: [
      { modelId: 'off', enabled: false },
      { modelId: 'ready-model', enabled: true },
    ],
  },
]

describe('selectDefaultModel', () => {
  it('selects the first enabled model without depending on a global active provider', () => {
    expect(selectDefaultModel(providers)).toEqual({
      provider: providers[1],
      model: providers[1].models[1],
    })
  })

  it('returns null when no provider has an enabled model', () => {
    expect(selectDefaultModel(providers.slice(0, 1))).toBeNull()
  })
})
