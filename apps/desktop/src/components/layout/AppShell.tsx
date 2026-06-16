import { useState } from 'react'

import { ProviderSettings } from '../ProviderSettings'
import { useChatStreamListener } from '../../hooks/useChatStreamListener'
import { useSessionStore } from '../../stores/sessionStore'
import { ChatView } from '../../views/ChatView'
import { Sidebar } from './Sidebar'
import type { AppView } from './types'

export function AppShell() {
  const [view, setView] = useState<AppView>('chat')
  const [sidebarOpen, setSidebarOpen] = useState(true)
  const activeSessionId = useSessionStore((state) => state.activeSessionId)

  // 全局单订阅 chat-stream-event(生命周期 = app)。
  useChatStreamListener()

  return (
    <main
      className={`grid h-screen overflow-hidden bg-paper text-ink ${
        sidebarOpen ? 'grid-cols-[320px_minmax(0,1fr)]' : 'grid-cols-[90px_minmax(0,1fr)]'
      } max-[760px]:grid-cols-1`}
    >
      <Sidebar
        view={view}
        expanded={sidebarOpen}
        onToggleExpanded={() => setSidebarOpen((open) => !open)}
        onNavigate={setView}
      />
      <section className="h-screen min-w-0 overflow-hidden max-[760px]:h-[calc(100vh-90px)]">
        {view === 'chat' ? (
          <ChatView sessionId={activeSessionId} />
        ) : (
          <div className="h-screen overflow-auto bg-paper p-6 max-[760px]:h-[calc(100vh-90px)]">
            <ProviderSettings onBack={() => setView('chat')} />
          </div>
        )}
      </section>
    </main>
  )
}
