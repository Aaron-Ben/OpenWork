import { invoke } from '@tauri-apps/api/core'

import type { ChatGenerateResponse, ChatGenerateStreamRequest } from '../type/chat'
import type { Session, SessionInput, SessionLoadResult, SessionSummary } from '../type/session'

// 会话与聊天(agent loop)的 Tauri invoke 封装。
export const sessionsApi = {
  list: (): Promise<SessionSummary[]> => invoke('session_list'),
  create: (input: SessionInput): Promise<Session> => invoke('session_create', { input }),
  load: (id: string): Promise<SessionLoadResult> => invoke('session_load', { id }),
  remove: (id: string): Promise<void> => invoke('session_delete', { id }),
  rename: (id: string, title: string): Promise<Session> => invoke('session_rename', { id, title }),
  chatGenerateStream: (request: ChatGenerateStreamRequest): Promise<ChatGenerateResponse> =>
    invoke('chat_generate_stream', { request }),
}
