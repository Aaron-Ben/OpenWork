import { beforeEach, describe, expect, it, vi } from 'vitest'

import { collabCommands } from '@/bridge/collab'
import { usePermissionStore } from './permissionStore'

vi.mock('@/bridge/collab', () => ({
  collabCommands: { listPermissions: vi.fn(), replyPermission: vi.fn(), abortPermission: vi.fn() },
}))

describe('permissionStore', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    usePermissionStore.setState({ pending: [], loading: false, error: null })
  })

  it('passes reject feedback through and refreshes only the approval collection', async () => {
    vi.mocked(collabCommands.replyPermission).mockResolvedValue(undefined)
    vi.mocked(collabCommands.listPermissions).mockResolvedValue([])
    await usePermissionStore.getState().reply('per_1', 'reject', 'Use another path')
    expect(collabCommands.replyPermission).toHaveBeenCalledWith('per_1', 'reject', 'Use another path')
    expect(usePermissionStore.getState().pending).toEqual([])
  })
})
