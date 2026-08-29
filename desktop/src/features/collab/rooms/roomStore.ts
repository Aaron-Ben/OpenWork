import { create } from 'zustand'

import { collabCommands, type CollabRoom } from '@/bridge/collab'
import { resolveErrorMessage } from '@/lib/commandError'

export function totalUnread(_rooms: CollabRoom[]): number {
  return 0
}

interface RoomStoreState {
  rooms: CollabRoom[]
  loading: boolean
  error: string | null
  fetchAll: () => Promise<void>
}

export const useRoomStore = create<RoomStoreState>((set) => ({
  rooms: [],
  loading: false,
  error: null,
  fetchAll: async () => {
    set({ loading: true, error: null })
    try {
      set({ rooms: await collabCommands.listRooms(), loading: false })
    } catch (error) {
      set({ error: resolveErrorMessage(error), loading: false })
    }
  },
}))
