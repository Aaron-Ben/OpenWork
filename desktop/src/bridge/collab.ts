import { invoke } from '@tauri-apps/api/core'

export interface CollabRuntimeStatus {
  runtimeSessionId: string
  startedAt: number
  lastComputerHeartbeat: number | null
  engines: CollabEngineInventory[]
  engineReadiness: CollabEngineReadiness[]
  runners: CollabRunnerStatus[]
}

export interface CollabEngineInventory {
  engineId: string
  status: 'unknown' | 'ready' | 'missing' | 'error'
  version: string | null
  checkedAt: number
  lastError: string | null
  observedSessionId: string
}

export interface CollabEngineReadiness {
  engineId: string
  status: 'unknown' | 'ready' | 'missing' | 'error'
}

export interface CollabRunnerStatus {
  agentId: string
  configRevision: number
  state: 'running' | 'error'
  lastError: string | null
}

export interface CollabAgent {
  id: string
  displayName: string
  role: string | null
  persona: string
  engineId: 'opencode'
  mainModelId: string
  triageModelId: string
  configRevision: number
  agendaEnabled: boolean
  archivedAt: string | null
}

export interface CollabAgentInput {
  displayName: string
  role: string | null
  persona: string
  engineId: 'opencode'
  mainModelId: string
  triageModelId: string
}

export interface CollabRoom {
  id: string
  kind: 'direct' | 'group'
  title: string | null
}

export interface CollabParticipant {
  id: string
  kind: 'user' | 'agent'
  displayName: string
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
  boardId: string
  columnId: string
  title: string
  description: string | null
  position: number
  assigneeId: string | null
  createdBy: string
}

export interface CollabBoardColumn {
  id: string
  title: string
  position: number
  isTerminal: boolean
  cards: CollabCard[]
}

export interface CollabBoard {
  id: string
  title: string
  description: string | null
  createdBy: string
  columns: CollabBoardColumn[]
}

export interface CollabRun {
  id: string
  agentId: string
  runtimeSessionId: string
  trigger: string
  status: string
  engineId: string
  mainModelId: string
  observedModelId: string | null
  outcome: string | null
  roomId: string | null
  focusCardId: string | null
  triggerReason: string | null
  errorCode: string | null
  errorMessage: string | null
  stage: string
  startedAt: string
  heartbeatAt: string
  endedAt: string | null
  durationMs: number
  inputTokens: number | null
  cachedInputTokens: number | null
  cacheCreationInputTokens: number | null
  outputTokens: number | null
  rateLimitPercent: number | null
  toolCalls: number
  eventCount: number
  inboxMessageCount: number
}

export interface CollabRunEvent {
  id: string
  source: 'server' | 'runner' | 'engine'
  kind: string
  level: 'info' | 'warning' | 'error'
  data: Record<string, unknown>
  createdAt: string
}

export interface CollabRunTrace {
  run: CollabRun
  events: CollabRunEvent[]
}

export interface CollabRunFilters {
  agentId?: string | null
  status?: string | null
  limit?: number
}

export const collabCommands = {
  status: (): Promise<CollabRuntimeStatus> => invoke('collab_status'),
  listAgents: (): Promise<CollabAgent[]> => invoke('collab_agent_list'),
  createAgent: (agent: CollabAgentInput): Promise<CollabAgent> =>
    invoke('collab_agent_create', {
      displayName: agent.displayName,
      role: agent.role,
      persona: agent.persona,
      engineId: agent.engineId,
      mainModelId: agent.mainModelId,
      triageModelId: agent.triageModelId,
    }),
  updateAgent: (agentId: string, agent: CollabAgentInput): Promise<CollabAgent> =>
    invoke('collab_agent_update', {
      input: {
        agentId,
        displayName: agent.displayName,
        role: agent.role,
        persona: agent.persona,
        engineId: agent.engineId,
        mainModelId: agent.mainModelId,
        triageModelId: agent.triageModelId,
      },
    }),
  setAgentAgenda: (agentId: string, enabled: boolean): Promise<CollabAgent> =>
    invoke('collab_agent_agenda_set', { agentId, enabled }),
  archiveAgent: (agentId: string): Promise<CollabAgent> =>
    invoke('collab_agent_archive', { agentId }),
  restoreAgent: (agentId: string): Promise<CollabAgent> =>
    invoke('collab_agent_restore', { agentId }),
  listRooms: (): Promise<CollabRoom[]> => invoke('collab_room_list'),
  createDirectRoom: (agentId: string): Promise<CollabRoom> =>
    invoke('collab_direct_room_create', { agentId }),
  createGroupRoom: (title: string, agentIds: string[]): Promise<CollabRoom> =>
    invoke('collab_group_room_create', { title, agentIds }),
  listRoomMembers: (roomId: string): Promise<CollabParticipant[]> =>
    invoke('collab_room_member_list', { roomId }),
  addGroupMember: (roomId: string, agentId: string): Promise<CollabParticipant[]> =>
    invoke('collab_group_member_add', { roomId, agentId }),
  removeGroupMember: (roomId: string, agentId: string): Promise<CollabParticipant[]> =>
    invoke('collab_group_member_remove', { roomId, agentId }),
  sendMessage: (roomId: string, body: string): Promise<CollabMessage> =>
    invoke('collab_message_send', { roomId, body }),
  listMessages: (roomId: string): Promise<CollabMessage[]> =>
    invoke('collab_message_list', { roomId }),
  listBoards: (): Promise<CollabBoard[]> => invoke('collab_board_list'),
  createBoard: (title: string, description: string | null = null): Promise<CollabBoard> =>
    invoke('collab_board_create', { title, description }),
  updateBoard: (
    boardId: string,
    title: string,
    description: string | null,
  ): Promise<CollabBoard> => invoke('collab_board_update', { boardId, title, description }),
  deleteBoard: (boardId: string): Promise<string> =>
    invoke('collab_board_delete', { boardId }),
  createBoardColumn: (
    boardId: string,
    title: string,
    isTerminal: boolean,
  ): Promise<CollabBoard> =>
    invoke('collab_board_column_create', { boardId, title, isTerminal }),
  updateBoardColumn: (
    columnId: string,
    title: string,
    isTerminal: boolean,
  ): Promise<CollabBoard> =>
    invoke('collab_board_column_update', { columnId, title, isTerminal }),
  moveBoardColumn: (
    columnId: string,
    beforeColumnId: string | null,
  ): Promise<CollabBoard> =>
    invoke('collab_board_column_move', { columnId, beforeColumnId }),
  deleteBoardColumn: (columnId: string): Promise<CollabBoard> =>
    invoke('collab_board_column_delete', { columnId }),
  assignCard: (cardId: string, assigneeId: string | null): Promise<CollabCard> =>
    invoke('collab_card_assign', { cardId, assigneeId }),
  deleteCard: (cardId: string): Promise<string> =>
    invoke('collab_card_delete', { cardId }),
  listRuns: ({ agentId = null, status = null, limit = 100 }: CollabRunFilters = {}): Promise<CollabRun[]> =>
    invoke('collab_run_list', { agentId, status, limit }),
  getRunTrace: (runId: string): Promise<CollabRunTrace> =>
    invoke('collab_run_trace', { runId }),
}
