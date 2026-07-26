import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'

import { AppShell } from './components/layout/AppShell'
import { useModelStore } from './features/models/modelStore'
import { useSessionStore } from './features/sessions/sessionStore'
import { useTheme } from './hooks/useTheme'
import { useTraceContentStore } from './stores/traceContentStore'

function App() {
  const { t } = useTranslation()
  const fetchProviders = useModelStore((state) => state.fetchAll)
  const fetchPresets = useModelStore((state) => state.fetchPresets)
  const fetchSessions = useSessionStore((state) => state.fetchAll)
  const tracePolicySyncState = useTraceContentStore((state) => state.syncState)
  const tracePolicyError = useTraceContentStore((state) => state.error)
  const initializeTracePolicy = useTraceContentStore((state) => state.initialize)

  useTheme()

  useEffect(() => {
    void initializeTracePolicy()
  }, [initializeTracePolicy])

  useEffect(() => {
    if (tracePolicySyncState !== 'ready') return
    void fetchProviders()
    void fetchPresets()
    void fetchSessions()
  }, [fetchProviders, fetchPresets, fetchSessions, tracePolicySyncState])

  if (tracePolicySyncState !== 'ready') {
    return (
      <main className="grid h-full place-items-center bg-paper p-6 text-ink">
        <div className="max-w-md text-center">
          <p className="text-sm text-ink-faint">
            {tracePolicySyncState === 'error'
              ? t('settings.traceContent.startupError')
              : t('settings.traceContent.startupSync')}
          </p>
          {tracePolicyError ? (
            <p className="mt-2 break-words text-xs text-status-danger-ink" role="alert">
              {tracePolicyError}
            </p>
          ) : null}
          {tracePolicySyncState === 'error' ? (
            <button
              type="button"
              className="mt-4 rounded-lg bg-clay px-4 py-2 text-sm font-medium text-white"
              onClick={() => void initializeTracePolicy()}
            >
              {t('settings.traceContent.retry')}
            </button>
          ) : null}
        </div>
      </main>
    )
  }

  return <AppShell />
}

export default App
