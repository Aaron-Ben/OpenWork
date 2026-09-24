import { create } from 'zustand'

export type CollabView = 'rooms' | 'whispers' | 'agents' | 'boards' | 'observability'

/** 打开房间时预填到输入框的文字（卡片详情的“在房间中讨论”，collaboration-desktop.md §9）。 */
export interface PendingDraft {
  roomId: string
  text: string
}

interface CollabNavigationState {
  view: CollabView
  activeRoomId: string | null
  /** “Agent 私聊”页选中的房间（collaboration-desktop.md §7.6）。 */
  activeWhisperId: string | null
  pendingDraft: PendingDraft | null
  navigate: (view: CollabView) => void
  selectRoom: (roomId: string, draft?: string) => void
  selectWhisper: (roomId: string) => void
  /** 取出并清空 `roomId` 的预填文字；没有时返回 `null`。 */
  takeDraft: (roomId: string) => string | null
}

export const useCollabNavigationStore = create<CollabNavigationState>((set, get) => ({
  view: 'rooms',
  activeRoomId: null,
  activeWhisperId: null,
  pendingDraft: null,
  navigate: (view) => set({ view }),
  selectRoom: (activeRoomId, draft) => set({
    activeRoomId,
    view: 'rooms',
    pendingDraft: draft === undefined ? null : { roomId: activeRoomId, text: draft },
  }),
  selectWhisper: (activeWhisperId) => set({ activeWhisperId, view: 'whispers' }),
  takeDraft: (roomId) => {
    const pending = get().pendingDraft
    if (pending?.roomId !== roomId) return null
    set({ pendingDraft: null })
    return pending.text
  },
}))
