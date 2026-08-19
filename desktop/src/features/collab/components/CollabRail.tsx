import { Bot, LayoutDashboard, LogOut, MessagesSquare, ScrollText } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { useModeStore } from '@/app/modeStore'
import { Button } from '@/components/ui/button'
import { isMacOS } from '@/lib/platform'
import {
  useCollabNavigationStore,
  type CollabView,
} from '@/features/collab/collabNavigationStore'

export function railTopInsetClass(macOS: boolean): string {
  return macOS ? 'flex h-7 shrink-0 items-center justify-center' : 'hidden'
}

interface CollabRailProps {
  view: CollabView
  unreadCount: number
  permissionCount: number
  macOS?: boolean
}

export function CollabRail({
  view,
  unreadCount,
  permissionCount,
  macOS = isMacOS,
}: CollabRailProps) {
  const { t } = useTranslation()
  const navigate = useCollabNavigationStore((state) => state.navigate)
  const setMode = useModeStore((state) => state.setMode)
  const items: Array<{ view: CollabView; label: string; icon: React.ReactNode }> = [
    { view: 'rooms', label: t('collab.nav.rooms'), icon: <MessagesSquare size={20} /> },
    { view: 'agents', label: t('collab.nav.agents'), icon: <Bot size={20} /> },
    { view: 'boards', label: t('collab.nav.boards'), icon: <LayoutDashboard size={20} /> },
    { view: 'logs', label: t('collab.nav.logs'), icon: <ScrollText size={20} /> },
  ]

  return (
    <aside className="flex h-full w-20 shrink-0 flex-col border-r border-line bg-paper-hover">
      <div
        data-tauri-drag-region={macOS ? 'deep' : undefined}
        data-macos-traffic-light-inset={macOS || undefined}
        className={railTopInsetClass(macOS)}
        aria-hidden="true"
      />
      <div className="flex flex-1 flex-col items-center gap-2 px-2 py-2">
        {items.map((item) => (
          <div key={item.view} className="relative">
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className={`size-11 rounded-2xl ${view === item.view ? 'bg-paper text-clay shadow-sm' : ''}`}
              aria-label={item.label}
              aria-current={view === item.view ? 'page' : undefined}
              onClick={() => navigate(item.view)}
            >
              {item.icon}
            </Button>
            {item.view === 'rooms' && unreadCount > 0 ? (
              <Badge dataName="collab-unread" count={unreadCount} tone="clay" />
            ) : null}
            {item.view === 'rooms' && permissionCount > 0 ? (
              <Badge dataName="collab-approvals" count={permissionCount} tone="danger" lower />
            ) : null}
          </div>
        ))}
      </div>
      <div className="flex justify-center border-t border-line p-2">
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="size-11 rounded-2xl"
          aria-label={t('collab.backToWorkbench')}
          title={t('collab.backToWorkbench')}
          onClick={() => setMode('workbench')}
        >
          <LogOut size={20} />
        </Button>
      </div>
    </aside>
  )
}

function Badge({ dataName, count, tone, lower = false }: {
  dataName: 'collab-unread' | 'collab-approvals'
  count: number
  tone: 'clay' | 'danger'
  lower?: boolean
}) {
  return (
    <span
      {...{ [`data-${dataName}`]: count }}
      className={`absolute -right-1 ${lower ? 'bottom-0' : '-top-1'} min-w-4 rounded-full px-1 text-center text-[10px] font-semibold text-white ${tone === 'clay' ? 'bg-clay' : 'bg-red-600'}`}
    >
      {count > 99 ? '99+' : count}
    </span>
  )
}
