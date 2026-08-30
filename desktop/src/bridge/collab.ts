import { invoke } from '@tauri-apps/api/core'

export interface CollabAgent {
  id: string
  displayName: string
  systemPrompt: string
  engineId: 'opencode'
  model: string
  configVersion: number
  enabled: boolean
  scannerEnabled: boolean
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

export interface CollabCard {
  id: string
  title: string
  description: string | null
  position: number
  assigneeId: string | null
  claimedBy: string | null
}

export interface CollabBoardColumn {
  id: string
  title: string
  position: number
  isDone: boolean
  cards: CollabCard[]
}

export interface CollabBoard {
  id: string
  roomId: string
  title: string
  columns: CollabBoardColumn[]
}

export interface CollabRun {
  id: string
  agentId: string
  trigger: string
  status: string
  outcome: string | null
  roomId: string | null
  focusCardId: string | null
  triggerReason: string | null
  startedAt: string
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
  setAgentProactivity: (agentId: string, enabled: boolean): Promise<CollabAgent> =>
    invoke('collab_agent_proactivity_set', { agentId, enabled }),
  listRooms: (): Promise<CollabRoom[]> => invoke('collab_room_list'),
  createDirectRoom: (agentId: string): Promise<CollabRoom> =>
    invoke('collab_direct_room_create', { agentId }),
  sendMessage: (roomId: string, body: string): Promise<CollabMessage> =>
    invoke('collab_message_send', { roomId, body }),
  listMessages: (roomId: string): Promise<CollabMessage[]> =>
    invoke('collab_message_list', { roomId }),
  listBoards: (): Promise<CollabBoard[]> => invoke('collab_board_list'),
  createBoard: (roomId: string, title: string): Promise<CollabBoard> =>
    invoke('collab_board_create', { roomId, title }),
  listRuns: (limit = 50): Promise<CollabRun[]> => invoke('collab_run_list', { limit }),
}
