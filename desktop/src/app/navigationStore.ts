import { create } from 'zustand'

import type { AppView } from '@/app/types'

/** 中栏当前展开的子智能体。null 表示中栏是主控对话。 */
export interface AgentFocus {
  parentSessionId: string
  childSessionId: string
}

interface NavigationStoreState {
  view: AppView
  sidebarExpanded: boolean
  agentRailExpanded: boolean
  agentFocus: AgentFocus | null
  messageFocus: { sessionId: string; messageId: string; requestId: number } | null
  messageFocusSequence: number
  navigate: (view: AppView) => void
  requestMessageFocus: (sessionId: string, messageId: string) => void
  clearMessageFocus: (requestId: number) => void
  setSidebarExpanded: (expanded: boolean) => void
  toggleSidebar: () => void
  setAgentRailExpanded: (expanded: boolean) => void
  focusAgent: (parentSessionId: string, childSessionId: string) => void
  clearAgentFocus: () => void
}

export function shouldCollapseSidebar(viewportWidth: number): boolean {
  return viewportWidth < 720
}

/** 三栏在窄窗口里挤不下：右栏先让位，左栏更晚才收。 */
export function shouldCollapseAgentRail(viewportWidth: number): boolean {
  return viewportWidth < 1100
}

export const useNavigationStore = create<NavigationStoreState>((set) => ({
  view: 'chat',
  sidebarExpanded: true,
  agentRailExpanded: true,
  agentFocus: null,
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
  setAgentRailExpanded: (agentRailExpanded) => set({ agentRailExpanded }),
  focusAgent: (parentSessionId, childSessionId) => set({
    agentFocus: { parentSessionId, childSessionId },
  }),
  clearAgentFocus: () => set((state) => state.agentFocus === null ? state : { agentFocus: null }),
}))
