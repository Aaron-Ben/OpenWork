import { useState } from 'react'
import {
  ArrowLeft,
  Bot,
  Check,
  MessageSquare,
  PanelLeftClose,
  Palette,
  Pencil,
  Plus,
  Settings as SettingsIcon,
  Trash2,
  X,
} from 'lucide-react'
import { AnimatePresence, motion, useReducedMotion } from 'motion/react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { useActiveProvider } from '../../stores/providerStore'
import { useSessionStore } from '../../stores/sessionStore'
import type { SessionSummary } from '../../type/session'
import type { AppView } from './types'

interface SidebarProps {
  view: AppView
  expanded: boolean
  onToggleExpanded: () => void
  onNavigate: (next: AppView) => void
}

export function Sidebar({ view, expanded, onToggleExpanded, onNavigate }: SidebarProps) {
  const { t } = useTranslation()
  const sessions = useSessionStore((state) => state.sessions)
  const activeSessionId = useSessionStore((state) => state.activeSessionId)
  const select = useSessionStore((state) => state.select)
  const create = useSessionStore((state) => state.create)
  const rename = useSessionStore((state) => state.rename)
  const remove = useSessionStore((state) => state.remove)
  const active = useActiveProvider()
  const reduceMotion = useReducedMotion()

  async function handleCreate() {
    if (!active) return
    const model = active.models.find((item) => item.enabled)?.modelId
    if (!model) return
    onNavigate('chat')
    await create({ providerId: active.id, model, title: t('sidebar.untitledSession') })
  }

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

  if (view !== 'chat') {
    return (
      <motion.aside
        data-motion-sidebar="true"
        data-settings-sidebar="true"
        className="flex h-full shrink-0 flex-col overflow-hidden border-r border-line bg-paper-hover"
        initial={false}
        animate={{ width: 240 }}
        transition={reduceMotion ? { duration: 0 } : { duration: 0.22, ease: 'easeOut' }}
      >
        <div className="flex h-16 shrink-0 items-center gap-3 px-3">
          <div className="grid size-10 shrink-0 place-items-center rounded-xl bg-ink font-sans text-sm font-bold text-paper">OW</div>
          <div className="min-w-0 flex-1 font-sans text-lg font-semibold text-ink">{t('settings.title')}</div>
          <Button type="button" variant="ghost" size="icon" className="size-9 rounded-xl" aria-label={t('sidebar.collapse')} aria-expanded="true" onClick={onToggleExpanded}>
            <PanelLeftClose size={19} />
          </Button>
        </div>
        <div className="px-3 pb-5 pt-1">
          <Button type="button" variant="ghost" className="h-10 w-full justify-start rounded-xl px-3" onClick={() => onNavigate('chat')}>
            <ArrowLeft size={18} />
            {t('sidebar.backToApp')}
          </Button>
        </div>
        <nav className="min-h-0 flex-1 px-3" aria-label={t('sidebar.settingsNavigation')}>
          <div className="px-3 pb-2 font-sans text-xs font-medium text-ink-faint">{t('settings.title')}</div>
          <SettingsNavItem active={view === 'settings-models'} icon={<Bot size={18} />} onClick={() => onNavigate('settings-models')}>
            {t('settings.models.title')}
          </SettingsNavItem>
          <SettingsNavItem active={view === 'settings-appearance'} icon={<Palette size={18} />} onClick={() => onNavigate('settings-appearance')}>
            {t('settings.appearance.title')}
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
      <div className="flex h-16 shrink-0 items-center gap-3 px-3">
        <div className="grid size-10 shrink-0 place-items-center rounded-xl bg-ink font-sans text-sm font-bold tracking-tight text-paper">
          OW
        </div>
        <motion.div
          className="min-w-0 flex-1 font-sans text-lg font-semibold tracking-tight text-ink"
          initial={reduceMotion ? false : { opacity: 0, x: -6 }}
          animate={{ opacity: 1, x: 0 }}
          transition={{ duration: 0.14 }}
        >
          {t('sidebar.brand')}
        </motion.div>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="size-9 shrink-0 rounded-xl"
          aria-label={t('sidebar.collapse')}
          aria-expanded="true"
          title={t('sidebar.collapse')}
          onClick={onToggleExpanded}
        >
          <PanelLeftClose size={19} />
        </Button>
      </div>

      <div className="px-3 pb-3 pt-1">
        <Button
          type="button"
          variant="ghost"
          className={`h-10 w-full rounded-xl text-ink ${expanded ? 'justify-start px-3' : 'px-0'}`}
          aria-label={t('sidebar.newSession')}
          title={t('sidebar.newSession')}
          disabled={!active}
          onClick={() => void handleCreate()}
        >
          <Plus size={19} className="shrink-0" />
          {expanded ? <span>{t('sidebar.newSession')}</span> : null}
        </Button>
      </div>

      <div className="min-h-0 flex-1 overflow-hidden">
        <AnimatePresence initial={false}>
          {expanded ? (
            <motion.div
              className="flex h-full w-[240px] flex-col px-3"
              initial={reduceMotion ? false : { opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.14 }}
            >
              <div className="px-2 pb-2 pt-1 font-sans text-xs font-medium text-ink-faint">{t('sidebar.sessions')}</div>
              <div className="min-h-0 flex-1 overflow-y-auto pb-4">
                <div className="grid gap-1">
                  {sessions.map((session) => (
                    <SessionItem
                      key={session.id}
                      session={session}
                      active={activeSessionId === session.id && view === 'chat'}
                      onSelect={() => {
                        onNavigate('chat')
                        void select(session.id)
                      }}
                      onRename={(title) => void rename(session.id, title)}
                      onDelete={() => void remove(session.id)}
                    />
                  ))}
                  {sessions.length === 0 ? (
                    <div className="px-2 py-4 font-sans text-xs text-ink-faint">{t('sidebar.emptySessions')}</div>
                  ) : null}
                </div>
              </div>
            </motion.div>
          ) : null}
        </AnimatePresence>
      </div>

      <div data-sidebar-footer="true" className="shrink-0 border-t border-line p-3">
        <Button
          type="button"
          variant="ghost"
          className="h-10 w-full justify-start rounded-xl px-3"
          aria-label={t('sidebar.settings')}
          title={t('sidebar.settings')}
          onClick={() => onNavigate('settings-models')}
        >
          <SettingsIcon size={19} className="shrink-0" />
          {expanded ? <span>{t('sidebar.settings')}</span> : null}
        </Button>
      </div>
    </motion.aside>
  )
}

type ItemMode = 'view' | 'edit' | 'confirm-delete'

interface SessionItemProps {
  session: SessionSummary
  active: boolean
  onSelect: () => void
  onRename: (title: string) => void
  onDelete: () => void
}

function SessionItem({ session, active, onSelect, onRename, onDelete }: SessionItemProps) {
  const { t } = useTranslation()
  const [mode, setMode] = useState<ItemMode>('view')
  const [draft, setDraft] = useState(session.title)

  function commitRename() {
    const title = draft.trim()
    if (title && title !== session.title) onRename(title)
    else setDraft(session.title)
    setMode('view')
  }

  if (mode === 'edit') {
    return (
      <form
        className="flex items-center gap-1 rounded-lg bg-paper px-2 py-1.5"
        onSubmit={(event) => {
          event.preventDefault()
          commitRename()
        }}
      >
        <input
          autoFocus
          value={draft}
          aria-label={t('sidebar.sessionName')}
          className="min-w-0 flex-1 rounded-md border border-line bg-paper px-2 py-1 text-sm outline-none focus:border-clay"
          onChange={(event) => setDraft(event.target.value)}
          onBlur={commitRename}
          onKeyDown={(event) => {
            if (event.key === 'Escape') {
              setDraft(session.title)
              setMode('view')
            }
          }}
        />
        <Button type="submit" variant="ghost" size="icon" className="size-6 text-emerald-600" aria-label={t('common.confirm')}>
          <Check size={13} />
        </Button>
      </form>
    )
  }

  if (mode === 'confirm-delete') {
    return (
      <div className="flex items-center gap-1 rounded-lg bg-rose-50 px-2 py-2">
        <span className="min-w-0 flex-1 truncate font-sans text-xs text-rose-700">{t('sidebar.deleteSessionPrompt')}</span>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="size-6 text-rose-600"
          aria-label={t('common.confirm')}
          onClick={() => {
            onDelete()
            setMode('view')
          }}
        >
          <Check size={13} />
        </Button>
        <Button type="button" variant="ghost" size="icon" className="size-6" aria-label={t('common.cancel')} onClick={() => setMode('view')}>
          <X size={13} />
        </Button>
      </div>
    )
  }

  return (
    <div className="group relative">
      <button
        type="button"
        onClick={onSelect}
        className={`flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-left font-sans text-sm transition ${
          active ? 'bg-paper font-medium text-ink shadow-sm' : 'text-ink-soft hover:bg-paper hover:text-ink'
        }`}
      >
        <MessageSquare size={16} className={`shrink-0 ${active ? 'text-clay' : 'text-ink-faint'}`} />
        <span className="min-w-0 flex-1 truncate pr-12">{session.title}</span>
      </button>
      <div className="absolute right-1 top-1/2 flex -translate-y-1/2 gap-0.5 opacity-0 transition group-hover:opacity-100 group-focus-within:opacity-100">
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="size-6"
          aria-label={t('sidebar.renameSession')}
          onClick={() => {
            setDraft(session.title)
            setMode('edit')
          }}
        >
          <Pencil size={12} />
        </Button>
        <Button type="button" variant="ghost" size="icon" className="size-6 hover:text-rose-500" aria-label={t('sidebar.deleteSession')} onClick={() => setMode('confirm-delete')}>
          <Trash2 size={12} />
        </Button>
      </div>
    </div>
  )
}

function SettingsNavItem({ active, icon, children, onClick }: { active: boolean; icon: React.ReactNode; children: React.ReactNode; onClick: () => void }) {
  return (
    <Button
      type="button"
      variant="ghost"
      className={`mb-1 h-10 w-full justify-start rounded-xl px-3 ${active ? 'bg-paper text-ink shadow-sm' : ''}`}
      onClick={onClick}
    >
      {icon}
      {children}
    </Button>
  )
}
