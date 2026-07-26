import { lazy, Suspense, useEffect, useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { AppearanceSettings } from '../settings/AppearanceSettings'
import { ContextWindowSettings } from '../settings/ContextWindowSettings'
import { TraceContentSettings } from '../settings/TraceContentSettings'
import { useCoreEventBridge } from '../../app/useCoreEventBridge'
import { shouldCollapseSidebar, useNavigationStore } from '../../app/navigationStore'
import { resyncSessionView } from '../../app/coreEventController'
import { ChatPage } from '../../features/chat/ChatPage'
import { ModelSettings } from '../../features/models/components/ModelSettings'
import { useSessionStore } from '../../features/sessions/sessionStore'
import { normalizeDirectoryPath, useProjectStore } from '../../stores/projectStore'
import { MainHeader } from './MainHeader'
import { Sidebar } from './Sidebar'

const TracePage = lazy(() => import('../../features/traces/components/TracePage'))

export function AppShell() {
  const { t } = useTranslation()
  const view = useNavigationStore((state) => state.view)
  const sidebarOpen = useNavigationStore((state) => state.sidebarExpanded)
  const navigate = useNavigationStore((state) => state.navigate)
  const setSidebarOpen = useNavigationStore((state) => state.setSidebarExpanded)
  const toggleSidebar = useNavigationStore((state) => state.toggleSidebar)
  const activeSessionId = useSessionStore((state) => state.activeSessionId)
  const orderedSessionIds = useSessionStore((state) => state.orderedSessionIds)
  const sessionSummaries = useSessionStore((state) => state.summaries)
  const sessions = useMemo(
    () => orderedSessionIds.flatMap((id) => sessionSummaries[id] ? [sessionSummaries[id]] : []),
    [orderedSessionIds, sessionSummaries],
  )
  const selectSession = useSessionStore((state) => state.select)
  const clearSessionSelection = useSessionStore((state) => state.clearSelection)
  const activeProjectPath = useProjectStore((state) => state.activeProjectPath)
  const activeSessionTitle = useSessionStore(
    (state) => state.activeSessionId ? state.summaries[state.activeSessionId]?.title ?? null : null,
  )

  useCoreEventBridge()

  useEffect(() => {
    function collapseSidebarForNarrowWindow() {
      if (shouldCollapseSidebar(window.innerWidth)) setSidebarOpen(false)
    }

    collapseSidebarForNarrowWindow()
    window.addEventListener('resize', collapseSidebarForNarrowWindow)
    return () => window.removeEventListener('resize', collapseSidebarForNarrowWindow)
  }, [setSidebarOpen])

  useEffect(() => {
    if (activeSessionId) void resyncSessionView(activeSessionId)
  }, [activeSessionId])

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
        onToggleExpanded={toggleSidebar}
        onNavigate={navigate}
      />
      <section className="flex h-full min-w-0 flex-1 flex-col overflow-hidden">
        <MainHeader
          title={
            view === 'chat'
              ? activeSessionTitle
              : view === 'traces'
                ? t('activity.title')
                : view === 'settings-models'
                  ? t('settings.models.title')
                  : view === 'settings-context'
                    ? t('settings.contextWindow.title')
                    : view === 'settings-trace'
                      ? t('settings.traceContent.title')
                      : t('settings.appearance.title')
          }
          kind={view === 'chat' ? 'session' : view === 'traces' ? 'activity' : 'settings'}
          sidebarExpanded={sidebarOpen}
          onToggleSidebar={() => setSidebarOpen(true)}
        />
        <div className="min-h-0 flex-1 overflow-hidden">
          {view === 'chat' ? (
            <ChatPage sessionId={activeSessionId} />
          ) : view === 'traces' ? (
            <Suspense fallback={<div className="p-8 text-sm text-ink-faint">{t('activity.loading')}</div>}>
              <TracePage />
            </Suspense>
          ) : view === 'settings-models' ? (
            <div className="h-full overflow-auto bg-paper">
              <ModelSettings />
            </div>
          ) : view === 'settings-context' ? (
            <div className="h-full overflow-auto bg-paper">
              <ContextWindowSettings />
            </div>
          ) : view === 'settings-trace' ? (
            <div className="h-full overflow-auto bg-paper">
              <TraceContentSettings />
            </div>
          ) : (
            <div className="h-full overflow-auto bg-paper">
              <AppearanceSettings />
            </div>
          )}
        </div>
      </section>
    </main>
  )
}
