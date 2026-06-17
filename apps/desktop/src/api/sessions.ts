import { invoke } from '@tauri-apps/api/core'

import type { ChatGenerateResponse, ChatGenerateStreamRequest } from '../type/chat'
import type {
  RevertSnapshotResponse,
  Session,
  SessionInput,
  SessionLoadResult,
  SessionSummary,
  WorktreeSnapshotDetail,
  WorktreeSnapshotSummary,
} from '../type/session'

// 会话与聊天(agent loop)的 Tauri invoke 封装。
export const sessionsApi = {
  list: (): Promise<SessionSummary[]> => invoke('session_list'),
  create: (input: SessionInput): Promise<Session> => invoke('session_create', { input }),
  load: (id: string): Promise<SessionLoadResult> => invoke('session_load', { id }),
  remove: (id: string): Promise<void> => invoke('session_delete', { id }),
  rename: (id: string, title: string): Promise<Session> => invoke('session_rename', { id, title }),
  worktreeSnapshots: (sessionId: string): Promise<WorktreeSnapshotSummary[]> =>
    invoke('session_worktree_snapshots', { sessionId }),
  worktreeSnapshotDetail: (snapshotId: string): Promise<WorktreeSnapshotDetail> =>
    invoke('session_worktree_snapshot_detail', { snapshotId }),
  revertWorktreeSnapshot: (snapshotId: string): Promise<RevertSnapshotResponse> =>
    invoke('session_revert_worktree_snapshot', { snapshotId }),
  chatGenerateStream: (request: ChatGenerateStreamRequest): Promise<ChatGenerateResponse> =>
    invoke('chat_generate_stream', { request }),
  chatAbort: (requestId: string): Promise<boolean> => invoke('chat_abort', { requestId }),
}
