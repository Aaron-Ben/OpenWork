import { Activity, Bot, ClipboardList, Eye, MessagesSquare, Settings } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { isMacOS } from '@/lib/platform'
import { useCollabNavigationStore, type CollabView } from '@/features/collab/collabNavigationStore'

export function CollabRail({ view, macOS = isMacOS }: { view: CollabView; macOS?: boolean }) {
  const { t } = useTranslation()
  const navigate = useCollabNavigationStore((state) => state.navigate)
  const items: Array<{ view: CollabView; label: string; icon: React.ReactNode }> = [
    { view: 'rooms', label: t('collab.nav.rooms'), icon: <MessagesSquare size={20} /> },
    { view: 'whispers', label: t('collab.nav.whispers'), icon: <Eye size={20} /> },
    { view: 'agents', label: t('collab.nav.agents'), icon: <Bot size={20} /> },
    { view: 'boards', label: t('collab.nav.boards'), icon: <ClipboardList size={20} /> },
    { view: 'observability', label: t('collab.nav.observability'), icon: <Activity size={20} /> },
  ]
  return (
    <aside className="flex h-full w-20 shrink-0 flex-col border-r border-line bg-paper-hover">
      <div data-tauri-drag-region={macOS ? 'deep' : undefined} className={macOS ? 'h-7 shrink-0' : 'hidden'} />
      <div className="flex flex-1 flex-col items-center gap-2 px-2 py-2">
        {items.map((item) => (
          <Button key={item.view} type="button" variant="ghost" size="icon" className={`size-11 rounded-2xl ${view === item.view ? 'bg-paper text-clay-ink shadow-sm' : ''}`} aria-label={item.label} onClick={() => navigate(item.view)}>
            {item.icon}
          </Button>
        ))}
      </div>
      <div className="flex justify-center border-t border-line p-2">
        <Button type="button" variant="ghost" size="icon" className={`size-11 rounded-2xl ${view === 'settings' ? 'bg-paper text-clay-ink shadow-sm' : ''}`} aria-label={t('collab.nav.settings')} onClick={() => navigate('settings')}><Settings size={20} /></Button>
      </div>
    </aside>
  )
}
