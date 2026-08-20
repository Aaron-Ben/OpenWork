import { beforeEach, describe, expect, it, vi } from 'vitest'

import { collabCommands } from '@/bridge/collab'
import { providersApi } from '@/bridge/providers'
import { selectTriageModelOptions, useTriageStore } from './triageStore'

vi.mock('@/bridge/collab', () => ({
  collabCommands: { getTriageSettings: vi.fn(), configureTriage: vi.fn() },
}))

vi.mock('@/bridge/providers', () => ({
  providersApi: { list: vi.fn() },
}))

const providerIndex = {
  providers: [
    {
      id: 'enabled-provider', name: 'Enabled', baseUrl: 'https://example.com', kind: 'openai' as const,
      enabled: true,
      models: [
        { modelId: 'enabled-model', displayName: 'Enabled Model', modelTier: 'lite' as const, enabled: true },
        { modelId: 'disabled-model', modelTier: 'pro' as const, enabled: false },
      ],
    },
    {
      id: 'disabled-provider', name: 'Disabled', baseUrl: 'https://example.com', kind: 'openai' as const,
      enabled: false,
      models: [
        { modelId: 'hidden-model', modelTier: 'lite' as const, enabled: true },
      ],
    },
  ],
}

describe('triageStore', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useTriageStore.setState({
      settings: null,
      providers: [],
      loading: false,
      saving: false,
      error: null,
    })
  })

  it('offers only enabled models from enabled OpenWork providers', () => {
    expect(selectTriageModelOptions(providerIndex.providers)).toEqual([
      {
        providerId: 'enabled-provider',
        providerName: 'Enabled',
        modelId: 'enabled-model',
        modelName: 'Enabled Model',
      },
    ])
  })

  it('loads an unconfigured fail-open state alongside the available models', async () => {
    vi.mocked(collabCommands.getTriageSettings).mockResolvedValue(null)
    vi.mocked(providersApi.list).mockResolvedValue(providerIndex)

    await useTriageStore.getState().fetch()

    expect(useTriageStore.getState()).toMatchObject({
      settings: null,
      providers: providerIndex.providers,
      loading: false,
      error: null,
    })
  })

  it('keeps provider choices when reading the current triage setting fails', async () => {
    vi.mocked(collabCommands.getTriageSettings).mockRejectedValue(
      new Error('stale daemon protocol'),
    )
    vi.mocked(providersApi.list).mockResolvedValue(providerIndex)

    await useTriageStore.getState().fetch()

    expect(selectTriageModelOptions(useTriageStore.getState().providers)).toHaveLength(1)
    expect(useTriageStore.getState()).toMatchObject({
      settings: null,
      loading: false,
      error: 'stale daemon protocol',
    })
  })

  it('saves and exposes the new triage model immediately', async () => {
    vi.mocked(collabCommands.configureTriage).mockResolvedValue({
      providerId: 'enabled-provider',
      modelId: 'enabled-model',
    })

    const saved = await useTriageStore.getState().save('enabled-provider', 'enabled-model')

    expect(saved).toBe(true)
    expect(collabCommands.configureTriage).toHaveBeenCalledWith('enabled-provider', 'enabled-model')
    expect(useTriageStore.getState()).toMatchObject({
      settings: { providerId: 'enabled-provider', modelId: 'enabled-model' },
      saving: false,
      error: null,
    })
  })
})
