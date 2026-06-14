import { create } from 'zustand'

import { providersApi } from '../api/providers'
import type {
  ProviderConfig,
  ProviderIndex,
  ProviderInput,
  ProviderPreset,
  TestResult,
} from '../type/providers'

interface ProviderStoreState {
  providers: ProviderConfig[]
  activeId: string | null
  presets: ProviderPreset[]
  isLoading: boolean
  isPresetsLoading: boolean
  error: string | null

  fetchAll: () => Promise<void>
  fetchPresets: () => Promise<void>
  create: (input: ProviderInput) => Promise<ProviderConfig>
  update: (id: string, input: ProviderInput) => Promise<ProviderConfig>
  remove: (id: string) => Promise<void>
  activate: (id: string) => Promise<void>
  test: (config: ProviderConfig, model: string) => Promise<TestResult>
}

export const useProviderStore = create<ProviderStoreState>((set, get) => ({
  providers: [],
  activeId: null,
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
        activeId: index.activeId,
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

  activate: async (id) => {
    await providersApi.activate(id)
    await get().fetchAll()
  },

  test: async (config, model) => providersApi.test(config, model),
}))

export function useActiveProvider(): ProviderConfig | null {
  const { providers, activeId } = useProviderStore()
  if (!activeId) return null
  return providers.find((provider) => provider.id === activeId) ?? null
}

// Tauri reject 通常传 string(Rust Err(String)),也可能是 Error。
function resolveErrorMessage(error: unknown): string {
  if (typeof error === 'string') return error
  if (error instanceof Error) return error.message
  return 'Unexpected error'
}
