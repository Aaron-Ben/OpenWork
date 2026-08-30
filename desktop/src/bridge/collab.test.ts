import { invoke } from '@tauri-apps/api/core'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { collabCommands } from './collab'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

describe('P0 collaboration command bridge', () => {
  beforeEach(() => vi.clearAllMocks())

  it('creates an OpenCode Agent without Provider or approval fields', async () => {
    vi.mocked(invoke).mockResolvedValue(null)
    await collabCommands.createAgent({
      id: 'helper',
      displayName: 'Helper',
      systemPrompt: 'Help the user.',
      model: 'opencode/hy3-free',
    })
    expect(invoke).toHaveBeenCalledWith('collab_agent_create', {
      id: 'helper',
      displayName: 'Helper',
      systemPrompt: 'Help the user.',
      model: 'opencode/hy3-free',
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
})
