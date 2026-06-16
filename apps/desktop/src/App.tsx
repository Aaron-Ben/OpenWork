import { useEffect } from 'react'

import { AppShell } from './components/layout/AppShell'
import { useTheme } from './hooks/useTheme'
import { useProviderStore } from './stores/providerStore'
import { useSessionStore } from './stores/sessionStore'

function App() {
  const fetchProviders = useProviderStore((state) => state.fetchAll)
  const fetchPresets = useProviderStore((state) => state.fetchPresets)
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
