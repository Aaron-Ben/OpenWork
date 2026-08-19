import { invoke } from '@tauri-apps/api/core'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { collabCommands } from './collab'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

describe('collaboration command bridge', () => {
  beforeEach(() => vi.clearAllMocks())

  it('keeps the user identity and daemon internals out of message sends', async () => {
    vi.mocked(invoke).mockResolvedValue(null)
    await collabCommands.sendMessage('general', 'hello @alice')
    expect(invoke).toHaveBeenCalledWith('collab_message_send', {
      roomId: 'general',
      body: 'hello @alice',
    })
  })

  it('encodes sequence pagination without exposing a directory', async () => {
    vi.mocked(invoke).mockResolvedValue(null)
    await collabCommands.messagePage('general', { kind: 'before', sequence: 42 }, 40)
    expect(invoke).toHaveBeenCalledWith('collab_message_page', {
      roomId: 'general',
      anchor: { kind: 'before', sequence: 42 },
      limit: 40,
    })
  })

  it('requests one bounded flat collaboration log timeline', async () => {
    vi.mocked(invoke).mockResolvedValue([])
    await collabCommands.listLogs('general', 300)
    expect(invoke).toHaveBeenCalledWith('collab_log_list', {
      roomId: 'general',
      limit: 300,
    })
  })
})
