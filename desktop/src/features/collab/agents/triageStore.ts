import { create } from 'zustand'

import {
  collabCommands,
  type CollabTriageSettings,
} from '@/bridge/collab'
import type { ProviderConfig } from '@/bridge/providerContracts'
import { providersApi } from '@/bridge/providers'
import { resolveErrorMessage } from '@/lib/commandError'

export interface TriageModelOption {
  providerId: string
  providerName: string
  modelId: string
  modelName: string
}

export function selectTriageModelOptions(
  providers: readonly ProviderConfig[],
): TriageModelOption[] {
  return providers.flatMap((provider) => {
    if (!provider.enabled) return []
    return provider.models.flatMap((model) => model.enabled ? [{
      providerId: provider.id,
      providerName: provider.name,
      modelId: model.modelId,
      modelName: model.displayName || model.modelId,
    }] : [])
  })
}

interface TriageStoreState {
  settings: CollabTriageSettings | null
  providers: ProviderConfig[]
  loading: boolean
  saving: boolean
  error: string | null
  fetch: () => Promise<void>
  save: (providerId: string, modelId: string) => Promise<boolean>
}

export const useTriageStore = create<TriageStoreState>((set) => ({
  settings: null,
  providers: [],
  loading: false,
  saving: false,
  error: null,
  fetch: async () => {
    set({ loading: true, error: null })
    const [settingsResult, providersResult] = await Promise.allSettled([
      collabCommands.getTriageSettings(),
      providersApi.list(),
    ])
    const errors = [settingsResult, providersResult].flatMap((result) => (
      result.status === 'rejected' ? [resolveErrorMessage(result.reason)] : []
    ))
    set({
      settings: settingsResult.status === 'fulfilled' ? settingsResult.value : null,
      providers: providersResult.status === 'fulfilled' ? providersResult.value.providers : [],
      loading: false,
      error: errors.length > 0 ? errors.join('\n') : null,
    })
  },
  save: async (providerId, modelId) => {
    set({ saving: true, error: null })
    try {
      const settings = await collabCommands.configureTriage(providerId, modelId)
      set({ settings, saving: false })
      return true
    } catch (error) {
      set({ saving: false, error: resolveErrorMessage(error) })
      return false
    }
  },
}))
