import { useEffect } from 'react'

import { AppShell } from '@/app/AppShell'
import { CollabShell } from '@/app/CollabShell'
import { useCoreEventBridge } from '@/app/useCoreEventBridge'
import { useModeStore } from '@/app/modeStore'
import { totalUnread, useRoomStore } from '@/features/collab/rooms/roomStore'
import { useModelStore } from '@/features/models/modelStore'
import { useSessionStore } from '@/features/sessions/sessionStore'
import { useTheme } from '@/app/useTheme'

function App() {
  const mode = useModeStore((state) => state.mode)
  const setMode = useModeStore((state) => state.setMode)
  const rooms = useRoomStore((state) => state.rooms)
  const fetchProviders = useModelStore((state) => state.fetchAll)
  const fetchPresets = useModelStore((state) => state.fetchPresets)
  const fetchSessions = useSessionStore((state) => state.fetchAll)

  useTheme()
  useCoreEventBridge()

  useEffect(() => {
    void fetchProviders()
    void fetchPresets()
    void fetchSessions()
  }, [fetchProviders, fetchPresets, fetchSessions])

  return mode === 'collab' ? <CollabShell /> : (
    <AppShell
      collabUnreadCount={totalUnread(rooms)}
      onOpenCollab={() => setMode('collab')}
    />
  )
}

export default App
