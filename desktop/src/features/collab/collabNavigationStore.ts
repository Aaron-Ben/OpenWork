import { create } from 'zustand'

export type CollabView = 'rooms' | 'agents' | 'boards' | 'logs'

interface CollabNavigationState {
  view: CollabView
  activeRoomId: string | null
  rosterExpanded: boolean
  navigate: (view: CollabView) => void
  selectRoom: (roomId: string) => void
  setRosterExpanded: (expanded: boolean) => void
}

export const useCollabNavigationStore = create<CollabNavigationState>((set) => ({
  view: 'rooms',
  activeRoomId: null,
  rosterExpanded: true,
  navigate: (view) => set({ view }),
  selectRoom: (activeRoomId) => set({ activeRoomId, view: 'rooms' }),
  setRosterExpanded: (rosterExpanded) => set({ rosterExpanded }),
}))
