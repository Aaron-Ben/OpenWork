import { useState } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import {
  ArrowLeft,
  Bot,
  Check,
  ChevronDown,
  ChevronRight,
  Folder,
  MessageSquare,
  MoreHorizontal,
  PanelLeftClose,
  Palette,
  Pencil,
  Plus,
  Settings as SettingsIcon,
  SquarePen,
  Trash2,
  X,
} from 'lucide-react'
import { AnimatePresence, motion, useReducedMotion } from 'motion/react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { useActiveProvider } from '../../stores/providerStore'
import {
  normalizeDirectoryPath,
  type OpenedProject,
  useProjectStore,
} from '../../stores/projectStore'
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
  const projects = useProjectStore((state) => state.projects)
  const activeProjectPath = useProjectStore((state) => state.activeProjectPath)
  const projectsExpanded = useProjectStore((state) => state.projectsExpanded)
  const openDirectory = useProjectStore((state) => state.openDirectory)
  const selectProject = useProjectStore((state) => state.selectProject)
  const removeProject = useProjectStore((state) => state.removeProject)
  const toggleProjects = useProjectStore((state) => state.toggleProjects)
  const active = useActiveProvider()
  const reduceMotion = useReducedMotion()
  const [isOpeningDirectory, setIsOpeningDirectory] = useState(false)

  async function handleCreate(project: OpenedProject) {
    if (!active) return
    const model = active.models.find((item) => item.enabled)?.modelId
    if (!model) return
    selectProject(project.path)
    onNavigate('chat')
    await create({
      providerId: active.id,
      model,
      title: t('sidebar.untitledSession'),
      workingDir: project.path,
    })
  }

  async function handleOpenDirectory() {
    if (isOpeningDirectory) return
    setIsOpeningDirectory(true)
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: t('sidebar.openFolder'),
      })
      if (typeof selected === 'string') {
        openDirectory(selected)
        onNavigate('chat')
      }
    } finally {
      setIsOpeningDirectory(false)
    }
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
              <div
                data-project-section="true"
                className="flex items-center gap-1 px-1 pb-2 pt-1"
              >
                <Button
                  type="button"
                  variant="ghost"
                  className="h-8 min-w-0 flex-1 justify-start gap-1 rounded-lg px-2 text-xs font-medium text-ink-faint"
                  aria-expanded={projectsExpanded}
                  onClick={toggleProjects}
                >
                  {projectsExpanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
                  <span>{t('sidebar.projects')}</span>
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="size-8 rounded-lg text-ink-faint"
                  aria-label={t('sidebar.openFolder')}
                  title={t('sidebar.openFolder')}
                  disabled={isOpeningDirectory}
                  onClick={() => void handleOpenDirectory()}
                >
                  <Plus size={17} />
                </Button>
              </div>
              <div className="min-h-0 flex-1 overflow-y-auto pb-4">
                <AnimatePresence initial={false}>
                  {projectsExpanded ? (
                    <motion.div
                      className="grid gap-1"
                      initial={reduceMotion ? false : { opacity: 0, y: -4 }}
                      animate={{ opacity: 1, y: 0 }}
                      exit={{ opacity: 0, y: -4 }}
                      transition={{ duration: reduceMotion ? 0 : 0.14 }}
                    >
                      {projects.map((project) => {
                        const projectSessions = sessions.filter(
                          (session) =>
                            normalizeDirectoryPath(session.workingDir ?? '') === project.path,
                        )
                        const projectActive = activeProjectPath === project.path
                        return (
                          <div key={project.path}>
                            <ProjectItem
                              project={project}
                              active={projectActive}
                              canCreateSession={Boolean(active)}
                              onSelect={() => {
                                selectProject(project.path)
                                onNavigate('chat')
                              }}
                              onRemove={() => removeProject(project.path)}
                              onCreateSession={() => void handleCreate(project)}
                            />
                            {projectActive ? (
                              <div className="ml-4 mt-1 grid gap-1 border-l border-line pl-2">
                                {projectSessions.map((session) => (
                                  <SessionItem
                                    key={session.id}
                                    session={session}
                                    active={activeSessionId === session.id}
                                    onSelect={() => {
                                      onNavigate('chat')
                                      void select(session.id)
                                    }}
                                    onRename={(title) => void rename(session.id, title)}
                                    onDelete={() => void remove(session.id)}
                                  />
                                ))}
                                {projectSessions.length === 0 ? (
                                  <div className="px-3 py-2 font-sans text-xs text-ink-faint">
                                    {t('sidebar.emptyProjectSessions')}
                                  </div>
                                ) : null}
                              </div>
                            ) : null}
                          </div>
                        )
                      })}
                      {projects.length === 0 ? (
                        <div className="px-3 py-4 font-sans text-xs leading-5 text-ink-faint">
                          {t('sidebar.emptyProjects')}
                        </div>
                      ) : null}
                    </motion.div>
                  ) : null}
                </AnimatePresence>
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

interface ProjectItemProps {
  project: OpenedProject
  active: boolean
  canCreateSession: boolean
  onSelect: () => void
  onRemove: () => void
  onCreateSession: () => void
}

export function ProjectItem({
  project,
  active,
  canCreateSession,
  onSelect,
  onRemove,
  onCreateSession,
}: ProjectItemProps) {
  const { t } = useTranslation()
  return (
    <div
      data-project-row="true"
      className={`group relative rounded-xl transition ${active ? 'bg-paper shadow-sm' : 'hover:bg-paper'}`}
    >
      <button
        type="button"
        className={`flex h-10 w-full items-center gap-2.5 rounded-xl px-3 pr-16 text-left text-sm transition ${
          active ? 'font-medium text-ink' : 'text-ink-soft group-hover:text-ink'
        }`}
        title={project.path}
        onClick={onSelect}
      >
        <Folder size={17} className="shrink-0 text-ink-faint" />
        <span className="min-w-0 flex-1 truncate">{project.name}</span>
      </button>
      <div
        className={`absolute right-1 top-1/2 flex -translate-y-1/2 items-center gap-0.5 transition ${
          active
            ? 'opacity-100'
            : 'opacity-0 group-hover:opacity-100 group-focus-within:opacity-100'
        }`}
      >
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="size-7 rounded-lg text-ink-faint"
              aria-label={t('sidebar.projectActions', { name: project.name })}
              title={t('sidebar.projectActions', { name: project.name })}
            >
              <MoreHorizontal size={15} />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" side="right" className="min-w-48">
            <DropdownMenuItem
              className="flex cursor-default items-center gap-2 px-3 py-2 text-sm text-rose-600 data-[highlighted]:bg-rose-50"
              onSelect={onRemove}
            >
              <X size={15} />
              {t('sidebar.removeProject')}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="size-7 rounded-lg text-ink-faint"
          aria-label={t('sidebar.newSessionInProject', { name: project.name })}
          title={t('sidebar.newSessionInProject', { name: project.name })}
          disabled={!canCreateSession}
          onClick={onCreateSession}
        >
          <SquarePen size={15} />
        </Button>
      </div>
    </div>
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
