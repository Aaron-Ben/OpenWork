import { create } from 'zustand'

export type CollabView = 'rooms' | 'agents'

interface CollabNavigationState {
  view: CollabView
  activeRoomId: string | null
  navigate: (view: CollabView) => void
  selectRoom: (roomId: string) => void
}

export const useCollabNavigationStore = create<CollabNavigationState>((set) => ({
  view: 'rooms',
  activeRoomId: null,
  navigate: (view) => set({ view }),
  selectRoom: (activeRoomId) => set({ activeRoomId, view: 'rooms' }),
}))
