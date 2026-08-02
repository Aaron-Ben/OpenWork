import { describe, expect, it } from 'vitest'

import type { ProviderConfig } from './contracts'
import { selectDefaultModel } from './modelStore'

const providers: ProviderConfig[] = [
  {
    id: 'disabled-provider',
    name: 'Disabled',
    baseUrl: 'https://disabled.invalid',
    kind: 'openai',
    enabled: false,
    models: [{ modelId: 'disabled-model', modelTier: 'plus', enabled: true }],
  },
  {
    id: 'ready-provider',
    name: 'Ready',
    baseUrl: 'https://ready.invalid',
    kind: 'deepseek',
    enabled: true,
    models: [
      { modelId: 'off', modelTier: 'lite', enabled: false },
      { modelId: 'ready-model', modelTier: 'plus', enabled: true },
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
