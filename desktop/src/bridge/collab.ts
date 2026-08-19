import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

export interface CollabAgent {
  id: string
  displayName: string
  role: string | null
  bio: string | null
  systemPrompt: string
  providerId: string
  modelId: string
  opencodeSessionId: string | null
  enabled: boolean
  activity: CollabAgentActivity
}

export type CollabAgentInput = Omit<CollabAgent, 'opencodeSessionId' | 'activity'>

export type CollabAgentActivity =
  | { kind: 'idle' }
  | { kind: 'busy' }
  | { kind: 'replying' }
  | { kind: 'compacting' }
  | { kind: 'executing'; detail: string }
  | { kind: 'unresponsive' }

export interface CollabRoomMember {
  id: string
  displayName: string
  kind: 'user' | 'agent'
  enabled: boolean
}

export interface CollabRoomSummary {
  id: string
  kind: 'group' | 'direct'
  title: string | null
  nextSequence: number
  lastReadSequence: number
  unreadCount: number
  muted: boolean
  members: CollabRoomMember[]
}

export interface CollabMessage {
  id: string
  roomId: string
  sequence: number
  authorId: string
  kind: 'normal' | 'system'
  body: string
  createdAt: string
}

export interface CollabMessagePage {
  messages: CollabMessage[]
  hasOlder: boolean
  hasNewer: boolean
}

export type CollabMessagePageAnchor = {
  kind: 'around' | 'before' | 'after'
  sequence: number
}

export interface CollabPendingPermission {
  id: string
  sessionId: string
  agentId: string | null
  permission: string
  patterns: string[]
}

export type CollabPermissionReply = 'once' | 'always' | 'reject'

export type CollabEvent =
  | { version: number; sequence: number; type: 'rooms_changed'; roomId: string }
  | { version: number; sequence: number; type: 'agents_changed' }
  | { version: number; sequence: number; type: 'permissions_changed' }
  | { version: number; sequence: number; type: 'engine_changed' }
  | {
      version: number
      sequence: number
      type: 'reply_held'
      agentId: string
      roomId: string
      peerSequence: number
    }
  | {
      version: number
      sequence: number
      type: 'agent_activity_changed'
      agentId: string
      activity: CollabAgentActivity
    }

export const COLLAB_EVENT_VERSION = 1
export const COLLAB_EVENT = 'openwork://collab-event'
export const COLLAB_EVENT_BATCH = 'openwork://collab-event-batch'

export const collabCommands = {
  status: (): Promise<unknown> => invoke('collab_status'),
  listAgents: (): Promise<CollabAgent[]> => invoke('collab_agent_list'),
  createAgent: (agent: CollabAgentInput): Promise<CollabAgent> =>
    invoke('collab_agent_create', { agent }),
  updateAgent: (agent: CollabAgentInput): Promise<CollabAgent> =>
    invoke('collab_agent_update', { agent }),
  listRooms: (): Promise<CollabRoomSummary[]> => invoke('collab_room_list'),
  createRoom: (id: string, title: string): Promise<unknown> =>
    invoke('collab_room_create', { id, title }),
  addMember: (roomId: string, participantId: string): Promise<unknown> =>
    invoke('collab_room_add_member', { roomId, participantId }),
  sendMessage: (roomId: string, body: string): Promise<unknown> =>
    invoke('collab_message_send', { roomId, body }),
  messagePage: (
    roomId: string,
    anchor: CollabMessagePageAnchor | null,
    limit = 40,
  ): Promise<CollabMessagePage> =>
    invoke('collab_message_page', { roomId, anchor, limit }),
  markRead: (roomId: string, throughSequence: number): Promise<unknown> =>
    invoke('collab_room_mark_read', { roomId, throughSequence }),
  listPermissions: (): Promise<CollabPendingPermission[]> =>
    invoke('collab_permission_list'),
  replyPermission: (
    id: string,
    reply: CollabPermissionReply,
    message?: string,
  ): Promise<unknown> => invoke('collab_permission_reply', { id, reply, message }),
  abortPermission: (id: string): Promise<unknown> =>
    invoke('collab_permission_abort', { id }),
}

export async function listenToCollabEvents(
  handler: (payload: CollabEvent) => void,
): Promise<UnlistenFn> {
  const [unlistenSingle, unlistenBatch] = await Promise.all([
    listen<CollabEvent>(COLLAB_EVENT, (event) => handler(event.payload)),
    listen<CollabEvent[]>(COLLAB_EVENT_BATCH, (event) => {
      for (const payload of event.payload) handler(payload)
    }),
  ])
  return () => {
    unlistenSingle()
    unlistenBatch()
  }
}
