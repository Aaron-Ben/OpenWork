import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { ProviderSettings } from '../ProviderSettings'
import { AppearanceSettings } from '../settings/AppearanceSettings'
import { TraceSettings } from '../settings/TraceSettings'
import { useRuntimeSessionUpdates } from '../../hooks/useRuntimeSessionUpdates'
import { useRuntimeSessionStore } from '../../stores/runtimeSessionStore'
import { normalizeDirectoryPath, useProjectStore } from '../../stores/projectStore'
import { ChatView } from '../../views/ChatView'
import { MainHeader } from './MainHeader'
import { Sidebar } from './Sidebar'
import type { AppView } from './types'

export function AppShell() {
  const { t } = useTranslation()
  const [view, setView] = useState<AppView>('chat')
  const [sidebarOpen, setSidebarOpen] = useState(true)
  const activeSessionId = useRuntimeSessionStore((state) => state.activeSessionId)
  const sessions = useRuntimeSessionStore((state) => state.sessions)
  const selectSession = useRuntimeSessionStore((state) => state.select)
  const clearSessionSelection = useRuntimeSessionStore((state) => state.clearSelection)
  const activeProjectPath = useProjectStore((state) => state.activeProjectPath)
  const activeSessionTitle = useRuntimeSessionStore(
    (state) => state.sessions.find((session) => session.id === state.activeSessionId)?.title ?? null,
  )

  // 全局单订阅 chat-stream-event(生命周期 = app)。
  useRuntimeSessionUpdates()

  useEffect(() => {
    if (!activeProjectPath) {
      if (activeSessionId) clearSessionSelection()
      return
    }
    const normalizedProject = normalizeDirectoryPath(activeProjectPath)
    const selected = sessions.find((session) => session.id === activeSessionId)
    if (selected && normalizeDirectoryPath(selected.workingDirectory) === normalizedProject) return

    const first = sessions.find(
      (session) => normalizeDirectoryPath(session.workingDirectory) === normalizedProject,
    )
    if (first) void selectSession(first.id)
    else if (activeSessionId) clearSessionSelection()
  }, [activeProjectPath, activeSessionId, clearSessionSelection, selectSession, sessions])

  return (
    <main className="flex h-full overflow-hidden bg-paper text-ink">
      <Sidebar
        view={view}
        expanded={sidebarOpen}
        onToggleExpanded={() => setSidebarOpen((open) => !open)}
        onNavigate={setView}
      />
      <section className="flex h-full min-w-0 flex-1 flex-col overflow-hidden">
        <MainHeader
          title={
            view === 'chat'
              ? activeSessionTitle
              : view === 'settings-models'
                ? t('settings.models.title')
                : view === 'settings-appearance'
                  ? t('settings.appearance.title')
                  : t('settings.trace.title')
          }
          sidebarExpanded={sidebarOpen}
          onToggleSidebar={() => setSidebarOpen(true)}
        />
        <div className="min-h-0 flex-1 overflow-hidden">
          {view === 'chat' ? (
            <ChatView sessionId={activeSessionId} />
          ) : view === 'settings-models' ? (
            <div className="h-full overflow-auto bg-paper">
              <ProviderSettings />
            </div>
          ) : view === 'settings-appearance' ? (
            <div className="h-full overflow-auto bg-paper">
              <AppearanceSettings />
            </div>
          ) : (
            <TraceSettings />
          )}
        </div>
      </section>
    </main>
  )
}
