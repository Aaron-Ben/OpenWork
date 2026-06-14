import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  ChatGenerateRequest,
  ChatGenerateResponse,
  ChatGenerateStreamRequest,
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
  test: (config: ProviderConfig, model: string): Promise<TestResult> =>
    invoke('provider_test', { config, model }),
  chatGenerate: (request: ChatGenerateRequest): Promise<ChatGenerateResponse> =>
    invoke('chat_generate', { request }),
  chatGenerateStream: (request: ChatGenerateStreamRequest): Promise<ChatGenerateResponse> =>
    invoke('chat_generate_stream', { request }),
  listenToChatStream: (
    handler: (payload: ChatStreamEventPayload) => void,
  ): Promise<UnlistenFn> =>
    listen<ChatStreamEventPayload>('chat-stream-event', (event) => handler(event.payload)),
}
