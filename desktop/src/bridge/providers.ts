import { invoke } from '@tauri-apps/api/core'
import type {
  ProviderConfig,
  ProviderIndex,
  ProviderInput,
  ProviderPreset,
  TestResult,
} from './providerContracts'

// 薄 Tauri invoke 封装,每个方法对应一个 Rust command。
export const providersApi = {
  list: (): Promise<ProviderIndex> => invoke('provider_list'),
  presets: (): Promise<ProviderPreset[]> => invoke('provider_presets'),
  /** `presetId` becomes the provider id; null lets Core generate a custom id. */
  create: (presetId: string, input: ProviderInput): Promise<ProviderConfig> =>
    invoke('provider_create', { presetId, input }),
  update: (id: string, input: ProviderInput): Promise<ProviderConfig> =>
    invoke('provider_update', { id, input }),
  remove: (id: string): Promise<void> => invoke('provider_delete', { id }),
  test: (id: string, model: string): Promise<TestResult> =>
    invoke('provider_test', { id, model }),
  testDraft: (input: ProviderInput, model: string): Promise<TestResult> =>
    invoke('provider_test', { input, model }),
}
