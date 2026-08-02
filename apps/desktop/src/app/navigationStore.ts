import { create } from 'zustand'

import type { AppView } from '@/components/layout/types'

interface NavigationStoreState {
  view: AppView
  sidebarExpanded: boolean
  messageFocus: { sessionId: string; messageId: string; requestId: number } | null
  messageFocusSequence: number
  navigate: (view: AppView) => void
  requestMessageFocus: (sessionId: string, messageId: string) => void
  clearMessageFocus: (requestId: number) => void
  setSidebarExpanded: (expanded: boolean) => void
  toggleSidebar: () => void
}

export function shouldCollapseSidebar(viewportWidth: number): boolean {
  return viewportWidth < 720
}

export const useNavigationStore = create<NavigationStoreState>((set) => ({
  view: 'chat',
  sidebarExpanded: true,
  messageFocus: null,
  messageFocusSequence: 0,
  navigate: (view) => set({ view }),
  requestMessageFocus: (sessionId, messageId) => set((state) => ({
    messageFocusSequence: state.messageFocusSequence + 1,
    messageFocus: {
      sessionId,
      messageId,
      requestId: state.messageFocusSequence + 1,
    },
  })),
  clearMessageFocus: (requestId) => set((state) => (
    state.messageFocus?.requestId === requestId ? { messageFocus: null } : state
  )),
  setSidebarExpanded: (sidebarExpanded) => set({ sidebarExpanded }),
  toggleSidebar: () => set((state) => ({ sidebarExpanded: !state.sidebarExpanded })),
}))
