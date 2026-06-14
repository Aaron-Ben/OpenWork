import { useState } from 'react'

import { ProviderSettings } from '../ProviderSettings'
import { useActiveProvider } from '../../stores/providerStore'
import { ChatView } from '../../views/ChatView'
import { Sidebar } from './Sidebar'
import type { AppView } from './types'

export function AppShell() {
  const [view, setView] = useState<AppView>('chat')
  const [sidebarOpen, setSidebarOpen] = useState(true)
  const active = useActiveProvider()

  return (
    <main
      className={`grid h-screen overflow-hidden bg-zinc-50 text-slate-800 ${
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
          <ChatView activeId={active?.id ?? null} />
        ) : (
          <div className="h-screen overflow-auto bg-zinc-50 p-6 max-[760px]:h-[calc(100vh-90px)]">
            <ProviderSettings onBack={() => setView('chat')} />
          </div>
        )}
      </section>
    </main>
  )
}
