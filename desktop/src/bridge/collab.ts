import { invoke } from '@tauri-apps/api/core'

export interface CollabAgent {
  id: string
  displayName: string
  systemPrompt: string
  engineId: 'opencode'
  model: string
  configVersion: number
  enabled: boolean
}

export interface CollabAgentInput {
  id: string
  displayName: string
  systemPrompt: string
  model: string
}

export interface CollabRoom {
  id: string
  kind: 'direct' | 'group'
  title: string | null
}

export interface CollabMessage {
  id: string
  roomId: string
  sequence: number
  authorId: string
  body: string
}

export const collabCommands = {
  status: (): Promise<unknown> => invoke('collab_status'),
  listAgents: (): Promise<CollabAgent[]> => invoke('collab_agent_list'),
  createAgent: (agent: CollabAgentInput): Promise<CollabAgent> =>
    invoke('collab_agent_create', {
      id: agent.id,
      displayName: agent.displayName,
      systemPrompt: agent.systemPrompt,
      model: agent.model,
    }),
  listRooms: (): Promise<CollabRoom[]> => invoke('collab_room_list'),
  createDirectRoom: (agentId: string): Promise<CollabRoom> =>
    invoke('collab_direct_room_create', { agentId }),
  sendMessage: (roomId: string, body: string): Promise<CollabMessage> =>
    invoke('collab_message_send', { roomId, body }),
  listMessages: (roomId: string): Promise<CollabMessage[]> =>
    invoke('collab_message_list', { roomId }),
}
