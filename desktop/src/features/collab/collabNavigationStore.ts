import { create } from 'zustand'

export type CollabView = 'rooms' | 'whispers' | 'agents' | 'boards' | 'observability'

interface CollabNavigationState {
  view: CollabView
  activeRoomId: string | null
  /** “Agent 私聊”页选中的房间（collaboration-desktop.md §7.6）。 */
  activeWhisperId: string | null
  navigate: (view: CollabView) => void
  selectRoom: (roomId: string) => void
  selectWhisper: (roomId: string) => void
}

export const useCollabNavigationStore = create<CollabNavigationState>((set) => ({
  view: 'rooms',
  activeRoomId: null,
  activeWhisperId: null,
  navigate: (view) => set({ view }),
  selectRoom: (activeRoomId) => set({ activeRoomId, view: 'rooms' }),
  selectWhisper: (activeWhisperId) => set({ activeWhisperId, view: 'whispers' }),
}))
