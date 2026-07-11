import { FolderClosed, MoreHorizontal, PanelLeftOpen } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'

interface MainHeaderProps {
  title: string | null
  sidebarExpanded: boolean
  onToggleSidebar: () => void
}

export function MainHeader({ title, sidebarExpanded, onToggleSidebar }: MainHeaderProps) {
  const { t } = useTranslation()
  return (
    <header
      data-main-header="true"
      className="flex h-16 shrink-0 items-center gap-3 border-b border-line bg-paper px-5"
    >
      {!sidebarExpanded ? (
        <>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="size-9 shrink-0 rounded-xl"
            aria-label={t('sidebar.expand')}
            aria-expanded="false"
            title={t('sidebar.expand')}
            onClick={onToggleSidebar}
          >
            <PanelLeftOpen size={19} />
          </Button>
          <div className="h-6 w-px shrink-0 bg-line" />
        </>
      ) : null}
      <FolderClosed size={19} className="shrink-0 text-ink-soft" />
      <h1 className="min-w-0 truncate font-sans text-base font-semibold text-ink">
        {title?.trim() || t('sidebar.untitledSession')}
      </h1>
      <Button
        type="button"
        variant="ghost"
        size="icon"
        className="size-8 shrink-0 rounded-lg"
        aria-label={t('header.sessionActions')}
        title={t('header.sessionActions')}
      >
        <MoreHorizontal size={18} />
      </Button>
    </header>
  )
}
