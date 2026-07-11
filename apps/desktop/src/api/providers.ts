import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  ChatStreamEventPayload,
  ProviderConfig,
  ProviderIndex,
  ProviderInput,
  ProviderPreset,
  TestResult,
} from '../type/providers'

// 薄 Tauri invoke 封装,每个方法对应一个 Rust command。
export const providersApi = {
  list: (): Promise<ProviderIndex> => invoke('provider_list'),
  presets: (): Promise<ProviderPreset[]> => invoke('provider_presets'),
  create: (input: ProviderInput): Promise<ProviderConfig> =>
    invoke('provider_create', { input }),
  update: (id: string, input: ProviderInput): Promise<ProviderConfig> =>
    invoke('provider_update', { id, input }),
  remove: (id: string): Promise<void> => invoke('provider_delete', { id }),
  activate: (id: string): Promise<void> => invoke('provider_activate', { id }),
  test: (id: string, model: string): Promise<TestResult> =>
    invoke('provider_test', { id, model }),
  testDraft: (input: ProviderInput, model: string): Promise<TestResult> =>
    invoke('provider_test', { input, model }),
  resolveApproval: (turnId: string, approvalId: string, allow: boolean): Promise<void> =>
    invoke('resolve_approval', { turnId, approvalId, allow }),
  listenToChatStream: (
    handler: (payload: ChatStreamEventPayload) => void,
  ): Promise<UnlistenFn> =>
    listen<ChatStreamEventPayload>('chat-stream-event', (event) => handler(event.payload)),
}
