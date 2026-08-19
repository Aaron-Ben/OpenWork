import { create } from 'zustand'

import {
  collabCommands,
  type CollabPendingPermission,
  type CollabPermissionReply,
} from '@/bridge/collab'
import { resolveErrorMessage } from '@/lib/commandError'

interface PermissionStoreState {
  pending: CollabPendingPermission[]
  loading: boolean
  error: string | null
  fetchAll: () => Promise<void>
  reply: (id: string, reply: CollabPermissionReply, message?: string) => Promise<void>
  abort: (id: string) => Promise<void>
}

export const usePermissionStore = create<PermissionStoreState>((set, get) => ({
  pending: [],
  loading: false,
  error: null,
  fetchAll: async () => {
    set({ loading: true, error: null })
    try {
      set({ pending: await collabCommands.listPermissions(), loading: false })
    } catch (error) {
      set({ error: resolveErrorMessage(error), loading: false })
    }
  },
  reply: async (id, reply, message) => {
    await collabCommands.replyPermission(id, reply, message)
    await get().fetchAll()
  },
  abort: async (id) => {
    await collabCommands.abortPermission(id)
    await get().fetchAll()
  },
}))
