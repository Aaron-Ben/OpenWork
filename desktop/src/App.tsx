import { useEffect } from 'react'

import { AppShell } from '@/app/AppShell'
import { CollabShell } from '@/app/CollabShell'
import { useCoreEventBridge } from '@/app/useCoreEventBridge'
import { useCollabEventBridge } from '@/app/useCollabEventBridge'
import { useModeStore } from '@/app/modeStore'
import { usePermissionStore } from '@/features/collab/permissions/permissionStore'
import { totalUnread, useRoomStore } from '@/features/collab/rooms/roomStore'
import { useModelStore } from '@/features/models/modelStore'
import { useSessionStore } from '@/features/sessions/sessionStore'
import { useTheme } from '@/app/useTheme'

function App() {
  const mode = useModeStore((state) => state.mode)
  const setMode = useModeStore((state) => state.setMode)
  const rooms = useRoomStore((state) => state.rooms)
  const permissionCount = usePermissionStore((state) => state.pending.length)
  const fetchProviders = useModelStore((state) => state.fetchAll)
  const fetchPresets = useModelStore((state) => state.fetchPresets)
  const fetchSessions = useSessionStore((state) => state.fetchAll)

  useTheme()
  useCoreEventBridge()
  useCollabEventBridge()

  useEffect(() => {
    void fetchProviders()
    void fetchPresets()
    void fetchSessions()
  }, [fetchProviders, fetchPresets, fetchSessions])

  return mode === 'collab' ? <CollabShell /> : (
    <AppShell
      collabUnreadCount={totalUnread(rooms)}
      collabPermissionCount={permissionCount}
      onOpenCollab={() => setMode('collab')}
    />
  )
}

export default App
