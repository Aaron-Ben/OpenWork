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
  activity: CollabAgentActivity
}

/** Agent 现在在做什么（collaboration-desktop.md §4.1）；时间是带 `+08:00` 的 RFC 3339。 */
export type CollabAgentActivity =
  | {
    kind: 'working'
    roomId: string | null
    roomTitle: string | null
    cardId: string | null
    cardTitle: string | null
    startedAt: string
  }
  | { kind: 'queued', cardCount: number, firstCardTitle: string }
  | { kind: 'error', message: string }
  | { kind: 'idle', roomId: string | null, roomTitle: string | null, lastSpokeAt: string | null }
  | { kind: 'archived' }

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
  quoted: CollabQuotedMessage | null
}

/** 房间列表的一行（collaboration-desktop.md §4.2）；`memberIds` 中用户在最前。 */
export interface CollabRoomSummary {
  id: string
  kind: 'direct' | 'group'
  title: string | null
  unreadCount: number
  lastMessage: { authorName: string, body: string } | null
  lastMessageAt: string | null
  userIsMember: boolean
  memberIds: string[]
  pinned: boolean
}

/** 房间快照里的消息：带作者信息与时间。 */
export interface CollabRoomMessage extends CollabMessage {
  authorName: string
  authorKind: 'user' | 'agent'
  authorRole: string | null
  createdAt: string
}

/** 说明行（collaboration-desktop.md §7.3），显示在 `afterSequence` 那条消息之后。 */
export type CollabRoomNote =
  | { kind: 'routing', afterSequence: number, skippedNames: string[], targetNames: string[] }
  | { kind: 'lap_floor', afterSequence: number, speakerName: string }
  | { kind: 'loop_cap', afterSequence: number }

/** `collab_room_open` 的房间快照。 */
export interface CollabRoomSnapshot {
  roomId: string
  messages: CollabRoomMessage[]
  notes: CollabRoomNote[]
}

/** 被引用消息的摘要；`body` 最多 180 个字符（collaboration.md §9.3）。 */
export interface CollabQuotedMessage {
  id: string
  authorId: string
  authorName: string
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
  /** 负责人对这张卡片的当前状态（collaboration-desktop.md §4.3）；没有时 Rust 不发这个字段。 */
  agentState?: CollabCardAgentState
  /** 最近更新时间（带 `+08:00`），只在 Desktop 读取看板时有。 */
  updatedAt?: string
}

/** Desktop 卡片命令的结果：修改后的卡片与这次被叫醒的 Agent。 */
export interface CollabCardChange {
  card: CollabCard
  wokenAgentIds: string[]
}

export interface CollabCardInput {
  boardId: string
  columnId: string
  title: string
  description: string | null
  assigneeId: string | null
}

export type CollabCardAgentState = 'working' | 'queued'

/** Column 的语义（collaboration.md §11.1）；`null` 表示未分类。 */
export type CollabColumnKind = 'todo' | 'doing' | 'done'

export interface CollabBoardColumn {
  id: string
  title: string
  position: number
  kind: CollabColumnKind | null
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
  listRooms: (): Promise<CollabRoomSummary[]> => invoke('collab_room_list'),
  pinRoom: (roomId: string, pinned: boolean): Promise<void> =>
    invoke('collab_room_pin', { roomId, pinned }),
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
  sendMessage: (roomId: string, body: string, quotedMessageId: string | null): Promise<CollabMessage> =>
    invoke('collab_message_send', { roomId, body, quotedMessageId }),
  openRoom: (roomId: string): Promise<CollabRoomSnapshot> =>
    invoke('collab_room_open', { roomId }),
  markRoomViewed: (roomId: string, upToSeq: number): Promise<number> =>
    invoke('collab_room_viewed', { roomId, upToSeq }),
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
    kind: CollabColumnKind | null,
  ): Promise<CollabBoard> =>
    invoke('collab_board_column_create', { boardId, title, kind }),
  updateBoardColumn: (
    columnId: string,
    title: string,
    kind: CollabColumnKind | null,
  ): Promise<CollabBoard> =>
    invoke('collab_board_column_update', { columnId, title, kind }),
  moveBoardColumn: (
    columnId: string,
    beforeColumnId: string | null,
  ): Promise<CollabBoard> =>
    invoke('collab_board_column_move', { columnId, beforeColumnId }),
  deleteBoardColumn: (columnId: string): Promise<CollabBoard> =>
    invoke('collab_board_column_delete', { columnId }),
  createCard: (input: CollabCardInput): Promise<CollabCardChange> =>
    invoke('collab_card_create', { input }),
  /** `title` 与 `description` 至少给一个；`null` 表示保持原值，描述写空字符串即清空。 */
  updateCard: (cardId: string, title: string | null, description: string | null): Promise<CollabCardChange> =>
    invoke('collab_card_update', { cardId, title, description }),
  moveCard: (cardId: string, columnId: string, beforeCardId: string | null): Promise<CollabCardChange> =>
    invoke('collab_card_move', { cardId, columnId, beforeCardId }),
  assignCard: (cardId: string, assigneeId: string | null): Promise<CollabCardChange> =>
    invoke('collab_card_assign', { cardId, assigneeId }),
  deleteCard: (cardId: string): Promise<string> =>
    invoke('collab_card_delete', { cardId }),
  listRuns: ({ agentId = null, status = null, limit = 100 }: CollabRunFilters = {}): Promise<CollabRun[]> =>
    invoke('collab_run_list', { agentId, status, limit }),
  getRunTrace: (runId: string): Promise<CollabRunTrace> =>
    invoke('collab_run_trace', { runId }),
}
