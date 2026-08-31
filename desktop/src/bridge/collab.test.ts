import { invoke } from '@tauri-apps/api/core'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { collabCommands } from './collab'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

describe('R3 collaboration command bridge', () => {
  beforeEach(() => vi.clearAllMocks())

  it('creates an OpenCode Agent without Provider or approval fields', async () => {
    vi.mocked(invoke).mockResolvedValue(null)
    await collabCommands.createAgent({
      displayName: 'Helper',
      role: 'Researcher',
      persona: 'Help the user.',
      engineId: 'opencode',
      mainModelId: 'opencode/mimo-v2.5-free',
      triageModelId: 'opencode/mimo-v2.5-free',
    })
    expect(invoke).toHaveBeenCalledWith('collab_agent_create', {
      displayName: 'Helper',
      role: 'Researcher',
      persona: 'Help the user.',
      engineId: 'opencode',
      mainModelId: 'opencode/mimo-v2.5-free',
      triageModelId: 'opencode/mimo-v2.5-free',
    })
  })

  it('updates mutable Agent configuration without submitting a new id', async () => {
    vi.mocked(invoke).mockResolvedValue(null)
    await collabCommands.updateAgent('helper', {
      displayName: 'Updated Helper',
      role: null,
      persona: 'Use the revised persona.',
      engineId: 'opencode',
      mainModelId: 'opencode/main-v2',
      triageModelId: 'opencode/triage-v2',
    })
    expect(invoke).toHaveBeenCalledWith('collab_agent_update', {
      input: {
        agentId: 'helper',
        displayName: 'Updated Helper',
        role: null,
        persona: 'Use the revised persona.',
        engineId: 'opencode',
        mainModelId: 'opencode/main-v2',
        triageModelId: 'opencode/triage-v2',
      },
    })
  })

  it('keeps user identity and local paths out of message sends', async () => {
    vi.mocked(invoke).mockResolvedValue(null)
    await collabCommands.sendMessage('general', 'hello @helper')
    expect(invoke).toHaveBeenCalledWith('collab_message_send', {
      roomId: 'general',
      body: 'hello @helper',
    })
  })

  it('maps user group creation and membership changes to explicit control commands', async () => {
    vi.mocked(invoke).mockResolvedValue(null)

    await collabCommands.createGroupRoom('Launch room', ['alpha', 'beta'])
    await collabCommands.addGroupMember('room_1', 'gamma')
    await collabCommands.removeGroupMember('room_1', 'alpha')

    expect(invoke).toHaveBeenNthCalledWith(1, 'collab_group_room_create', {
      title: 'Launch room',
      agentIds: ['alpha', 'beta'],
    })
    expect(invoke).toHaveBeenNthCalledWith(2, 'collab_group_member_add', {
      roomId: 'room_1',
      agentId: 'gamma',
    })
    expect(invoke).toHaveBeenNthCalledWith(3, 'collab_group_member_remove', {
      roomId: 'room_1',
      agentId: 'alpha',
    })
  })

  it('maps R6 Board structure and Card ownership to Desktop-only commands', async () => {
    vi.mocked(invoke).mockResolvedValue(null)

    await collabCommands.updateBoard('board-1', 'Delivery', 'Shared work')
    await collabCommands.createBoardColumn('board-1', 'Review', false)
    await collabCommands.updateBoardColumn('column-1', 'Released', true)
    await collabCommands.moveBoardColumn('column-1', 'column-2')
    await collabCommands.assignCard('card-1', 'alpha')
    await collabCommands.deleteCard('card-1')

    expect(invoke).toHaveBeenNthCalledWith(1, 'collab_board_update', {
      boardId: 'board-1',
      title: 'Delivery',
      description: 'Shared work',
    })
    expect(invoke).toHaveBeenNthCalledWith(2, 'collab_board_column_create', {
      boardId: 'board-1',
      title: 'Review',
      isTerminal: false,
    })
    expect(invoke).toHaveBeenNthCalledWith(3, 'collab_board_column_update', {
      columnId: 'column-1',
      title: 'Released',
      isTerminal: true,
    })
    expect(invoke).toHaveBeenNthCalledWith(4, 'collab_board_column_move', {
      columnId: 'column-1',
      beforeColumnId: 'column-2',
    })
    expect(invoke).toHaveBeenNthCalledWith(5, 'collab_card_assign', {
      cardId: 'card-1',
      assigneeId: 'alpha',
    })
    expect(invoke).toHaveBeenNthCalledWith(6, 'collab_card_delete', {
      cardId: 'card-1',
    })
  })
})
