import { useEffect } from 'react'

import { AppShell } from './components/layout/AppShell'
import { useTheme } from './hooks/useTheme'
import { useProviderStore } from './stores/providerStore'

function App() {
  const fetchAll = useProviderStore((state) => state.fetchAll)
  const fetchPresets = useProviderStore((state) => state.fetchPresets)

  useTheme()

  useEffect(() => {
    void fetchAll()
    void fetchPresets()
  }, [fetchAll, fetchPresets])

  return <AppShell />
}

export default App
