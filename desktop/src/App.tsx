import { useEffect } from 'react'

import { AppShell } from '@/app/AppShell'
import { useModelStore } from '@/features/models/modelStore'
import { useSessionStore } from '@/features/sessions/sessionStore'
import { useTheme } from '@/app/useTheme'

function App() {
  const fetchProviders = useModelStore((state) => state.fetchAll)
  const fetchPresets = useModelStore((state) => state.fetchPresets)
  const fetchSessions = useSessionStore((state) => state.fetchAll)

  useTheme()

  useEffect(() => {
    void fetchProviders()
    void fetchPresets()
    void fetchSessions()
  }, [fetchProviders, fetchPresets, fetchSessions])

  return <AppShell />
}

export default App
