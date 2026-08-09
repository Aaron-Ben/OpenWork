import { Activity, ArrowLeft, Bot, PanelLeftClose, Puzzle, Settings as SettingsIcon, SlidersHorizontal } from 'lucide-react'
import { motion, useReducedMotion } from 'motion/react'
import { useTranslation } from 'react-i18next'

import { ProjectSection } from '@/features/sessions/components/ProjectSection'
import { isMacOS } from '@/lib/platform'
import { Button } from '@/components/ui/button'
import type { AppView } from './types'

export { ProjectItem } from '@/features/sessions/components/ProjectSection'

interface SidebarProps {
  view: AppView
  expanded: boolean
  onToggleExpanded: () => void
  onNavigate: (next: AppView) => void
}

export function Sidebar({ view, expanded, onToggleExpanded, onNavigate }: SidebarProps) {
  const { t } = useTranslation()
  const reduceMotion = useReducedMotion()

  if (!expanded) {
    return (
      <motion.aside
        data-motion-sidebar="true"
        aria-hidden="true"
        className="h-full shrink-0 overflow-hidden"
        initial={false}
        animate={{ width: 0 }}
        transition={reduceMotion ? { duration: 0 } : { duration: 0.22, ease: 'easeOut' }}
      />
    )
  }

  if (view.startsWith('settings-') || view === 'traces') {
    return (
      <motion.aside
        data-motion-sidebar="true"
        data-settings-sidebar="true"
        className="flex h-full shrink-0 flex-col overflow-hidden border-r border-line bg-paper-hover"
        initial={false}
        animate={{ width: 240 }}
        transition={reduceMotion ? { duration: 0 } : { duration: 0.22, ease: 'easeOut' }}
      >
        <SidebarHeader onCollapse={onToggleExpanded} />
        <div className="px-3 pb-5 pt-1">
          <Button type="button" variant="ghost" className="h-10 w-full justify-start rounded-xl px-3" onClick={() => onNavigate('chat')}>
            <ArrowLeft size={18} />{t('sidebar.backToApp')}
          </Button>
        </div>
        <nav className="min-h-0 flex-1 px-3" aria-label={t('sidebar.settingsNavigation')}>
          <div className="px-3 pb-2 font-sans text-xs font-medium text-ink-faint">{t('settings.title')}</div>
          <SettingsNavItem active={view === 'settings-models'} icon={<Bot size={18} />} onClick={() => onNavigate('settings-models')}>
            {t('settings.models.title')}
          </SettingsNavItem>
          <SettingsNavItem active={view === 'settings-skills'} icon={<Puzzle size={18} />} onClick={() => onNavigate('settings-skills')}>
            {t('settings.skills.title')}
          </SettingsNavItem>
          <SettingsNavItem active={view === 'settings-general'} icon={<SlidersHorizontal size={18} />} onClick={() => onNavigate('settings-general')}>
            {t('settings.general.title')}
          </SettingsNavItem>
          <SettingsNavItem
            active={view === 'traces'}
            activity
            icon={<Activity size={18} />}
            onClick={() => onNavigate('traces')}
          >
            {t('activity.navigation')}
          </SettingsNavItem>
        </nav>
      </motion.aside>
    )
  }

  return (
    <motion.aside
      data-motion-sidebar="true"
      className="flex h-full shrink-0 flex-col overflow-hidden border-r border-line bg-paper-hover"
      initial={false}
      animate={{ width: 240 }}
      transition={reduceMotion ? { duration: 0 } : { duration: 0.22, ease: 'easeOut' }}
    >
      <SidebarHeader title={t('sidebar.brand')} onCollapse={onToggleExpanded} />
      <div className="min-h-0 flex-1 overflow-hidden">
        <motion.div
          className="h-full"
          initial={reduceMotion ? false : { opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ duration: 0.14 }}
        >
          <ProjectSection
            onNavigateToChat={() => onNavigate('chat')}
            onNavigateToModels={() => onNavigate('settings-models')}
          />
        </motion.div>
      </div>
      <div data-sidebar-footer="true" className="grid shrink-0 gap-1 border-t border-line px-3 py-2">
        <Button
          type="button"
          variant="ghost"
          className="h-9 w-full justify-start rounded-xl px-3"
          aria-label={t('sidebar.settings')}
          title={t('sidebar.settings')}
          onClick={() => onNavigate('settings-models')}
        >
          <SettingsIcon size={17} className="shrink-0" />
          <span>{t('sidebar.settings')}</span>
        </Button>
      </div>
    </motion.aside>
  )
}

function SidebarHeader({ title, onCollapse }: { title?: string; onCollapse: () => void }) {
  const { t } = useTranslation()
  return (
    <div data-tauri-drag-region="deep" className="shrink-0">
      <div className={`flex h-11 items-center justify-end ${isMacOS ? 'pl-20 pr-3' : 'px-3'}`}>
        <Button type="button" variant="ghost" size="icon" className="size-9 rounded-xl" aria-label={t('sidebar.collapse')} aria-expanded="true" onClick={onCollapse}>
          <PanelLeftClose size={19} />
        </Button>
      </div>
      {title ? <div className="truncate px-4 pb-3 font-serif text-xl font-bold text-ink">{title}</div> : null}
    </div>
  )
}

function SettingsNavItem({ active, activity = false, icon, children, onClick }: {
  active: boolean
  activity?: boolean
  icon: React.ReactNode
  children: React.ReactNode
  onClick: () => void
}) {
  return (
    <Button
      type="button"
      variant="ghost"
      data-activity-navigation={activity || undefined}
      aria-current={active ? 'page' : undefined}
      className={`mb-1 h-10 w-full justify-start rounded-xl px-3 ${active ? 'bg-paper text-ink shadow-sm' : ''}`}
      onClick={onClick}
    >
      {icon}{children}
    </Button>
  )
}
