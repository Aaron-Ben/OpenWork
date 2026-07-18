import { create } from 'zustand'

import type { AppView } from '../components/layout/types'

interface NavigationStoreState {
  view: AppView
  sidebarExpanded: boolean
  navigate: (view: AppView) => void
  setSidebarExpanded: (expanded: boolean) => void
  toggleSidebar: () => void
}

export function shouldCollapseSidebar(viewportWidth: number): boolean {
  return viewportWidth < 720
}

export const useNavigationStore = create<NavigationStoreState>((set) => ({
  view: 'chat',
  sidebarExpanded: true,
  navigate: (view) => set({ view }),
  setSidebarExpanded: (sidebarExpanded) => set({ sidebarExpanded }),
  toggleSidebar: () => set((state) => ({ sidebarExpanded: !state.sidebarExpanded })),
}))
