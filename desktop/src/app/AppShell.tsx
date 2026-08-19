import { lazy, Suspense, useEffect, useMemo } from 'react'
import { AnimatePresence } from 'motion/react'
import { useTranslation } from 'react-i18next'

import { GeneralSettings } from '@/features/settings/components/GeneralSettings'
import { SkillSettings } from '@/features/settings/components/SkillSettings'
import {
  shouldCollapseAgentRail,
  shouldCollapseSidebar,
  useNavigationStore,
} from '@/app/navigationStore'
import { resyncSessionView } from '@/app/coreEventController'
import { ChatPage } from '@/features/chat/ChatPage'
import { AgentRail } from '@/features/chat/components/AgentRail'
import { SubAgentDetailPage } from '@/features/chat/SubAgentDetailPage'
import {
  agentRailElapsedMs,
  agentRailTotalSteps,
  buildAgentRailItems,
  findAgentRailItem,
} from '@/features/chat/agentRailModel'
import { formatSessionHeadline } from '@/features/chat/sessionHeadline'
import { useRuntimeStore } from '@/features/chat/runtimeStore'
import { useSubAgents } from '@/features/chat/useSubAgents'
import { useNowTicker } from '@/features/chat/useNowTicker'
import { ModelSettings } from '@/features/models/components/ModelSettings'
import { useSessionStore } from '@/features/sessions/sessionStore'
import { normalizeDirectoryPath, useProjectStore } from '@/features/projects/projectStore'
import { MainHeader } from './MainHeader'
import { Sidebar } from './Sidebar'

const TracePage = lazy(() => import('@/features/traces/TracePage'))

interface AppShellProps {
  collabUnreadCount: number
  collabPermissionCount: number
  onOpenCollab: () => void
}

export function AppShell({
  collabUnreadCount,
  collabPermissionCount,
  onOpenCollab,
}: AppShellProps) {
  const { t } = useTranslation()
  const view = useNavigationStore((state) => state.view)
  const sidebarOpen = useNavigationStore((state) => state.sidebarExpanded)
  const agentRailOpen = useNavigationStore((state) => state.agentRailExpanded)
  const agentFocus = useNavigationStore((state) => state.agentFocus)
  const navigate = useNavigationStore((state) => state.navigate)
  const setSidebarOpen = useNavigationStore((state) => state.setSidebarExpanded)
  const setAgentRailOpen = useNavigationStore((state) => state.setAgentRailExpanded)
  const toggleSidebar = useNavigationStore((state) => state.toggleSidebar)
  const focusAgent = useNavigationStore((state) => state.focusAgent)
  const clearAgentFocus = useNavigationStore((state) => state.clearAgentFocus)
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
  const projects = useProjectStore((state) => state.projects)
  const activeSessionTitle = useSessionStore(
    (state) => state.activeSessionId ? state.summaries[state.activeSessionId]?.title ?? null : null,
  )
  const runtimeBySession = useRuntimeStore((state) => state.bySession)
  const subAgents = useSubAgents(view === 'chat' ? activeSessionId : null)

  useEffect(() => {
    function collapsePanelsForNarrowWindow() {
      if (shouldCollapseSidebar(window.innerWidth)) setSidebarOpen(false)
      setAgentRailOpen(!shouldCollapseAgentRail(window.innerWidth))
    }

    collapsePanelsForNarrowWindow()
    window.addEventListener('resize', collapsePanelsForNarrowWindow)
    return () => window.removeEventListener('resize', collapsePanelsForNarrowWindow)
  }, [setAgentRailOpen, setSidebarOpen])

  useEffect(() => {
    if (activeSessionId) void resyncSessionView(activeSessionId)
  }, [activeSessionId])

  // 换会话就退回主控视图：子智能体属于某一棵树，跟着新会话显示旧的那棵是错的。
  useEffect(() => {
    if (agentFocus && agentFocus.parentSessionId !== activeSessionId) clearAgentFocus()
  }, [activeSessionId, agentFocus, clearAgentFocus])

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

  const activeSession = activeSessionId ? sessionSummaries[activeSessionId] : undefined
  const projectName = useMemo(() => {
    if (!activeSession) return null
    const directory = normalizeDirectoryPath(activeSession.workingDirectory)
    return projects.find((project) => project.path === directory)?.name ?? null
  }, [activeSession, projects])

  const sessionRunning = activeSessionId
    ? (runtimeBySession[activeSessionId]?.phase ?? 'idle') !== 'idle'
    : false
  const nowMs = useNowTicker(sessionRunning)
  const resolvedSessionTitle = activeSessionTitle?.trim() || t('sidebar.untitledSession')
  const railItems = useMemo(() => (
    activeSessionId
      ? buildAgentRailItems({
          parentSessionId: activeSessionId,
          orchestratorRole: t('chat.agents.orchestrator'),
          orchestratorTask: resolvedSessionTitle,
          children: subAgents.children,
          runtimeBySession,
          totalsBySession: subAgents.totalsBySession,
          nowMs,
        })
      : []
  ), [activeSessionId, nowMs, resolvedSessionTitle, runtimeBySession, subAgents, t])

  const focusedItem = findAgentRailItem(railItems, agentFocus?.childSessionId ?? null)
  const hasSubAgents = subAgents.children.length > 0
  const railVisible = view === 'chat' && agentRailOpen && hasSubAgents
  const subtitle = view === 'chat' && activeSessionId
    ? formatSessionHeadline(
        subAgents.children.length,
        agentRailTotalSteps(railItems),
        agentRailElapsedMs(railItems),
        t,
      )
    : null

  return (
    <main className="flex h-full overflow-hidden bg-paper text-ink">
      <Sidebar
        view={view}
        expanded={sidebarOpen}
        onToggleExpanded={toggleSidebar}
        onNavigate={navigate}
        collabUnreadCount={collabUnreadCount}
        collabPermissionCount={collabPermissionCount}
        onOpenCollab={onOpenCollab}
      />
      <section className="flex h-full min-w-0 flex-1 flex-col overflow-hidden">
        {view === 'chat' && focusedItem ? (
          <SubAgentDetailPage
            item={focusedItem}
            sessionTitle={resolvedSessionTitle}
            sidebarExpanded={sidebarOpen}
            onToggleSidebar={() => setSidebarOpen(true)}
            onBack={clearAgentFocus}
          />
        ) : (
          <>
            <MainHeader
              title={
                view === 'chat'
                  ? activeSessionTitle
                  : view === 'traces'
                    ? t('activity.title')
                    : view === 'settings-models'
                      ? t('settings.models.title')
                      : view === 'settings-skills'
                        ? t('settings.skills.title')
                      : t('settings.general.title')
              }
              kind={view === 'chat' ? 'session' : view === 'traces' ? 'activity' : 'settings'}
              projectName={view === 'chat' ? projectName : null}
              subtitle={subtitle}
              sidebarExpanded={sidebarOpen}
              onToggleSidebar={() => setSidebarOpen(true)}
              onRevealAgentRail={view === 'chat' && hasSubAgents && !agentRailOpen
                ? () => setAgentRailOpen(true)
                : undefined}
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
              ) : view === 'settings-skills' ? (
                <div className="h-full overflow-auto bg-paper">
                  <SkillSettings />
                </div>
              ) : (
                <div className="h-full overflow-auto bg-paper">
                  <GeneralSettings />
                </div>
              )}
            </div>
          </>
        )}
      </section>
      <AnimatePresence initial={false}>
        {railVisible ? (
          <AgentRail
            key="agent-rail"
            items={railItems}
            selectedSessionId={agentFocus?.childSessionId ?? null}
            error={subAgents.error}
            onCollapse={() => setAgentRailOpen(false)}
            onSelect={(sessionId) => {
              if (!activeSessionId) return
              if (agentFocus?.childSessionId === sessionId) clearAgentFocus()
              else focusAgent(activeSessionId, sessionId)
            }}
          />
        ) : null}
      </AnimatePresence>
    </main>
  )
}
