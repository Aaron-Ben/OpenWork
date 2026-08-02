import { create } from 'zustand'

import { providersApi } from '@/bridge/providers'
import { resolveErrorMessage } from '@/lib/commandError'
import type {
  ProviderConfig,
  ProviderIndex,
  ProviderInput,
  ProviderModel,
  ProviderPreset,
  TestResult,
} from './contracts'

export interface SelectedModel {
  provider: ProviderConfig
  model: ProviderModel
}

export function selectDefaultModel(providers: ProviderConfig[]): SelectedModel | null {
  for (const provider of providers) {
    if (!provider.enabled) continue
    const model = provider.models.find((candidate) => candidate.enabled)
    if (model) return { provider, model }
  }
  return null
}

interface ModelStoreState {
  providers: ProviderConfig[]
  presets: ProviderPreset[]
  isLoading: boolean
  isPresetsLoading: boolean
  error: string | null

  fetchAll: () => Promise<void>
  fetchPresets: () => Promise<void>
  create: (input: ProviderInput) => Promise<ProviderConfig>
  update: (id: string, input: ProviderInput) => Promise<ProviderConfig>
  remove: (id: string) => Promise<void>
  test: (config: ProviderConfig, model: string) => Promise<TestResult>
}

export const useModelStore = create<ModelStoreState>((set, get) => ({
  providers: [],
  presets: [],
  isLoading: false,
  isPresetsLoading: false,
  error: null,

  fetchAll: async () => {
    set({ isLoading: true, error: null })
    try {
      const index: ProviderIndex = await providersApi.list()
      set({
        providers: index.providers,
        isLoading: false,
      })
    } catch (error) {
      set({ isLoading: false, error: resolveErrorMessage(error) })
    }
  },

  fetchPresets: async () => {
    set({ isPresetsLoading: true })
    try {
      const presets = await providersApi.presets()
      set({ presets, isPresetsLoading: false })
    } catch (error) {
      set({ isPresetsLoading: false, error: resolveErrorMessage(error) })
    }
  },

  create: async (input) => {
    const config = await providersApi.create(input)
    await get().fetchAll()
    return config
  },

  update: async (id, input) => {
    const config = await providersApi.update(id, input)
    await get().fetchAll()
    return config
  },

  remove: async (id) => {
    await providersApi.remove(id)
    await get().fetchAll()
  },

  test: async (config, model) => providersApi.test(config.id, model),
}))
