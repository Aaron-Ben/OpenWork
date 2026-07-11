import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { ProviderSettings } from '../ProviderSettings'
import { AppearanceSettings } from '../settings/AppearanceSettings'
import { useChatStreamListener } from '../../hooks/useChatStreamListener'
import { useSessionStore } from '../../stores/sessionStore'
import { ChatView } from '../../views/ChatView'
import { MainHeader } from './MainHeader'
import { Sidebar } from './Sidebar'
import type { AppView } from './types'

export function AppShell() {
  const { t } = useTranslation()
  const [view, setView] = useState<AppView>('chat')
  const [sidebarOpen, setSidebarOpen] = useState(true)
  const activeSessionId = useSessionStore((state) => state.activeSessionId)
  const activeSessionTitle = useSessionStore(
    (state) => state.sessions.find((session) => session.id === state.activeSessionId)?.title ?? null,
  )

  // 全局单订阅 chat-stream-event(生命周期 = app)。
  useChatStreamListener()

  return (
    <main className="flex h-screen overflow-hidden bg-paper text-ink">
      <Sidebar
        view={view}
        expanded={sidebarOpen}
        onToggleExpanded={() => setSidebarOpen((open) => !open)}
        onNavigate={setView}
      />
      <section className="flex h-screen min-w-0 flex-1 flex-col overflow-hidden">
        <MainHeader
          title={
            view === 'chat'
              ? activeSessionTitle
              : view === 'settings-models'
                ? t('settings.models.title')
                : t('settings.appearance.title')
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
