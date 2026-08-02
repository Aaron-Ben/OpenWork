import { useMemo, useState } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import {
  Check,
  ChevronDown,
  ChevronRight,
  Folder,
  MessageSquare,
  MoreHorizontal,
  Pencil,
  Plus,
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
import { useProjectStore, normalizeDirectoryPath, type OpenedProject } from '@/features/projects/projectStore'
import type { RuntimeSessionRecord } from '@/bridge/compat'
import { selectDefaultModel, useModelStore } from '@/features/models/modelStore'
import { useRuntimeStore } from '@/features/chat/runtimeStore'
import type { SessionRuntimePhase } from '@/features/chat/runtimeReducer'
import { useSessionStore } from '../sessionStore'

export function ProjectSection({
  onNavigateToChat,
  onNavigateToModels,
}: {
  onNavigateToChat: () => void
  onNavigateToModels: () => void
}) {
  const { t } = useTranslation()
  const orderedSessionIds = useSessionStore((state) => state.orderedSessionIds)
  const sessionSummaries = useSessionStore((state) => state.summaries)
  const sessions = useMemo(
    () => orderedSessionIds.flatMap((id) => sessionSummaries[id] ? [sessionSummaries[id]] : []),
    [orderedSessionIds, sessionSummaries],
  )
  const activeSessionId = useSessionStore((state) => state.activeSessionId)
  const select = useSessionStore((state) => state.select)
  const create = useSessionStore((state) => state.create)
  const rename = useSessionStore((state) => state.rename)
  const remove = useSessionStore((state) => state.remove)
  const runtimeBySession = useRuntimeStore((state) => state.bySession)
  const projects = useProjectStore((state) => state.projects)
  const activeProjectPath = useProjectStore((state) => state.activeProjectPath)
  const projectsExpanded = useProjectStore((state) => state.projectsExpanded)
  const collapsedProjectPaths = useProjectStore((state) => state.collapsedProjectPaths)
  const openDirectory = useProjectStore((state) => state.openDirectory)
  const selectProject = useProjectStore((state) => state.selectProject)
  const removeProject = useProjectStore((state) => state.removeProject)
  const toggleProjects = useProjectStore((state) => state.toggleProjects)
  const toggleProject = useProjectStore((state) => state.toggleProject)
  const expandProject = useProjectStore((state) => state.expandProject)
  const providers = useModelStore((state) => state.providers)
  const reduceMotion = useReducedMotion()
  const [isOpeningDirectory, setIsOpeningDirectory] = useState(false)

  async function createSession(project: OpenedProject) {
    const selectedModel = selectDefaultModel(providers)
    if (!selectedModel) {
      onNavigateToModels()
      return
    }
    selectProject(project.path)
    expandProject(project.path)
    onNavigateToChat()
    await create({
      title: t('sidebar.untitledSession'),
      workingDirectory: project.path,
      provider: selectedModel.provider,
      modelId: selectedModel.model.modelId,
    })
  }

  async function chooseDirectory() {
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
        onNavigateToChat()
      }
    } finally {
      setIsOpeningDirectory(false)
    }
  }

  return (
    <div className="flex h-full w-[240px] flex-col px-3">
      <div data-project-section="true" className="flex items-center gap-1 px-1 pb-2 pt-1">
        <Button
          type="button"
          variant="ghost"
          className="h-8 min-w-0 flex-1 justify-start gap-1 rounded-lg px-0 text-sm font-medium text-ink-faint"
          aria-expanded={projectsExpanded}
          onClick={toggleProjects}
        >
          <span>{t('sidebar.projects')}</span>
          {projectsExpanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="size-8 rounded-lg text-ink-faint"
          aria-label={t('sidebar.openFolder')}
          title={t('sidebar.openFolder')}
          disabled={isOpeningDirectory}
          onClick={() => void chooseDirectory()}
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
                  (session) => normalizeDirectoryPath(session.workingDirectory) === project.path,
                )
                const projectActive = activeProjectPath === project.path
                const projectExpanded = projectActive && !collapsedProjectPaths.includes(project.path)
                return (
                  <div key={project.path}>
                    <ProjectItem
                      project={project}
                      active={projectActive}
                      expanded={projectExpanded}
                      onSelect={() => {
                        if (projectActive) toggleProject(project.path)
                        else {
                          selectProject(project.path)
                          expandProject(project.path)
                        }
                        onNavigateToChat()
                      }}
                      onRemove={() => removeProject(project.path)}
                      onCreateSession={() => void createSession(project)}
                    />
                    <AnimatePresence initial={false}>
                      {projectExpanded ? (
                        <motion.div
                          className="overflow-hidden"
                          initial={reduceMotion ? false : { height: 0, opacity: 0 }}
                          animate={{ height: 'auto', opacity: 1 }}
                          exit={{ height: 0, opacity: 0 }}
                          transition={{ duration: reduceMotion ? 0 : 0.16, ease: 'easeOut' }}
                        >
                          <div className="ml-4 mt-1 grid gap-1 border-l border-line pl-2">
                            {projectSessions.map((session) => (
                              <SessionItem
                                key={session.id}
                                session={session}
                                active={activeSessionId === session.id}
                                activity={runtimeBySession[session.id]?.phase ?? 'idle'}
                                onSelect={() => {
                                  onNavigateToChat()
                                  void select(session.id)
                                }}
                                onRename={(title) => void rename(session.id, title)}
                                onDelete={() => void remove(session.id)}
                              />
                            ))}
                            {projectSessions.length === 0 ? (
                              <div className="px-3 py-2 font-sans text-xs text-ink-faint">{t('sidebar.emptyProjectSessions')}</div>
                            ) : null}
                          </div>
                        </motion.div>
                      ) : null}
                    </AnimatePresence>
                  </div>
                )
              })}
              {projects.length === 0 ? (
                <div className="px-3 py-4 font-sans text-xs leading-5 text-ink-faint">{t('sidebar.emptyProjects')}</div>
              ) : null}
            </motion.div>
          ) : null}
        </AnimatePresence>
      </div>
    </div>
  )
}

interface ProjectItemProps {
  project: OpenedProject
  active: boolean
  expanded: boolean
  onSelect: () => void
  onRemove: () => void
  onCreateSession: () => void
}

export function ProjectItem({ project, active, expanded, onSelect, onRemove, onCreateSession }: ProjectItemProps) {
  const { t } = useTranslation()
  return (
    <div data-project-row="true" className={`group relative rounded-xl transition ${active ? 'bg-paper shadow-sm' : 'hover:bg-paper'}`}>
      <button type="button" className={`flex h-10 w-full items-center gap-2.5 rounded-xl px-3 pr-16 text-left text-sm transition ${active ? 'font-medium text-ink' : 'text-ink-soft group-hover:text-ink'}`} title={project.path} aria-expanded={expanded} onClick={onSelect}>
        {expanded ? <ChevronDown size={14} className="shrink-0 text-ink-faint" /> : <ChevronRight size={14} className="shrink-0 text-ink-faint" />}
        <Folder size={17} className="shrink-0 text-ink-faint" />
        <span className="min-w-0 flex-1 truncate">{project.name}</span>
      </button>
      <div className={`absolute right-1 top-1/2 flex -translate-y-1/2 items-center gap-0.5 transition ${active ? 'opacity-100' : 'opacity-0 group-hover:opacity-100 group-focus-within:opacity-100'}`}>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button type="button" variant="ghost" size="icon" className="size-7 rounded-lg text-ink-faint" aria-label={t('sidebar.projectActions', { name: project.name })} title={t('sidebar.projectActions', { name: project.name })}>
              <MoreHorizontal size={15} />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" side="right" className="min-w-48">
            <DropdownMenuItem className="flex cursor-default items-center gap-2 px-3 py-2 text-sm text-status-danger-ink data-[highlighted]:bg-status-danger-soft" onSelect={onRemove}>
              <X size={15} />{t('sidebar.removeProject')}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
        <Button type="button" variant="ghost" size="icon" className="size-7 rounded-lg text-ink-faint" aria-label={t('sidebar.newSessionInProject', { name: project.name })} title={t('sidebar.newSessionInProject', { name: project.name })} onClick={onCreateSession}>
          <SquarePen size={15} />
        </Button>
      </div>
    </div>
  )
}

type ItemMode = 'view' | 'edit' | 'confirm-delete'

function SessionItem({ session, active, activity, onSelect, onRename, onDelete }: {
  session: RuntimeSessionRecord
  active: boolean
  activity: SessionRuntimePhase
  onSelect: () => void
  onRename: (title: string) => void
  onDelete: () => void
}) {
  const { t } = useTranslation()
  const [mode, setMode] = useState<ItemMode>('view')
  const [draft, setDraft] = useState(session.title ?? '')
  function commitRename() {
    const title = draft.trim()
    if (title && title !== session.title) onRename(title)
    else setDraft(session.title ?? '')
    setMode('view')
  }
  if (mode === 'edit') {
    return (
      <form className="flex items-center gap-1 rounded-lg bg-paper px-2 py-1.5" onSubmit={(event) => { event.preventDefault(); commitRename() }}>
        <input autoFocus value={draft} aria-label={t('sidebar.sessionName')} className="min-w-0 flex-1 rounded-md border border-line bg-paper px-2 py-1 text-sm outline-none focus:border-clay" onChange={(event) => setDraft(event.target.value)} onBlur={commitRename} onKeyDown={(event) => { if (event.key === 'Escape') { setDraft(session.title ?? ''); setMode('view') } }} />
        <Button type="submit" variant="ghost" size="icon" className="size-6 text-status-success" aria-label={t('common.confirm')}><Check size={13} /></Button>
      </form>
    )
  }
  if (mode === 'confirm-delete') {
    return (
      <div className="flex items-center gap-1 rounded-lg bg-status-danger-soft px-2 py-2">
        <span className="min-w-0 flex-1 truncate font-sans text-xs text-status-danger-ink">{t('sidebar.deleteSessionPrompt')}</span>
        <Button type="button" variant="ghost" size="icon" className="size-6 text-status-danger-ink" aria-label={t('common.confirm')} onClick={() => { onDelete(); setMode('view') }}><Check size={13} /></Button>
        <Button type="button" variant="ghost" size="icon" className="size-6" aria-label={t('common.cancel')} onClick={() => setMode('view')}><X size={13} /></Button>
      </div>
    )
  }
  return (
    <div className="group relative">
      <button type="button" onClick={onSelect} aria-current={active ? 'page' : undefined} className={`flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-left font-sans text-sm transition ${active ? 'bg-paper font-medium text-ink shadow-sm' : 'text-ink-soft hover:bg-paper hover:text-ink'}`}>
        <span className="relative shrink-0">
          <MessageSquare size={16} className={active ? 'text-clay' : 'text-ink-faint'} />
          {activity !== 'idle' ? <span className={`absolute -right-1 -top-1 size-2 rounded-full ring-2 ring-paper-hover ${activity === 'waiting_permission' ? 'bg-status-warning' : 'bg-status-success'}`} title={activity === 'waiting_permission' ? t('activity.needsInput') : t('activity.running')} /> : null}
        </span>
        <span className="min-w-0 flex-1 truncate pr-12">{session.title}</span>
      </button>
      <div className="absolute right-1 top-1/2 flex -translate-y-1/2 gap-0.5 opacity-0 transition group-hover:opacity-100 group-focus-within:opacity-100">
        <Button type="button" variant="ghost" size="icon" className="size-6" aria-label={t('sidebar.renameSession')} onClick={() => { setDraft(session.title ?? ''); setMode('edit') }}><Pencil size={12} /></Button>
        <Button type="button" variant="ghost" size="icon" className="size-6 hover:text-status-danger-ink" aria-label={t('sidebar.deleteSession')} onClick={() => setMode('confirm-delete')}><Trash2 size={12} /></Button>
      </div>
    </div>
  )
}
