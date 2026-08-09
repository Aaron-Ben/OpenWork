import { useMemo, useState } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import { ChevronDown, ChevronRight, MoreHorizontal, Plus, SquarePen, X } from 'lucide-react'
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
import { selectDefaultModel, useModelStore } from '@/features/models/modelStore'
import { useRuntimeStore } from '@/features/chat/runtimeStore'
import { useSessionStore } from '../sessionStore'
import { SessionItem } from './SessionItem'

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
  const collapsedProjectPaths = useProjectStore((state) => state.collapsedProjectPaths)
  const openDirectory = useProjectStore((state) => state.openDirectory)
  const selectProject = useProjectStore((state) => state.selectProject)
  const removeProject = useProjectStore((state) => state.removeProject)
  const toggleProject = useProjectStore((state) => state.toggleProject)
  const expandProject = useProjectStore((state) => state.expandProject)
  const providers = useModelStore((state) => state.providers)
  const reduceMotion = useReducedMotion()
  const [isOpeningDirectory, setIsOpeningDirectory] = useState(false)
  const activeProject = projects.find((project) => project.path === activeProjectPath) ?? null

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
      <div data-new-conversation-row="true" className="shrink-0 pb-4 pt-1">
        <Button
          type="button"
          variant="accent"
          className="h-10 w-full justify-center gap-2 rounded-full px-3 text-sm font-semibold"
          disabled={!activeProject}
          title={activeProject ? t('sidebar.newSessionInProject', { name: activeProject.name }) : t('sidebar.newConversationHint')}
          onClick={() => {
            if (activeProject) void createSession(activeProject)
          }}
        >
          <Plus size={16} className="shrink-0" />
          <span className="min-w-0 truncate">{t('sidebar.newConversation')}</span>
        </Button>
      </div>

      <div data-project-section="true" className="min-h-0 flex-1 overflow-y-auto pb-3">
        <div className="px-2 pb-1 font-sans text-[11px] font-medium text-ink-faint">
          {t('sidebar.projects')}
        </div>
        <div className="grid gap-2">
          {projects.map((project) => {
            const projectSessions = sessions.filter(
              (session) => normalizeDirectoryPath(session.workingDirectory) === project.path,
            )
            const projectActive = activeProjectPath === project.path
            const projectExpanded = projectActive && !collapsedProjectPaths.includes(project.path)
            const projectRunning = projectSessions.some(
              (session) => (runtimeBySession[session.id]?.phase ?? 'idle') !== 'idle',
            )
            return (
              <div key={project.path}>
                <ProjectItem
                  project={project}
                  active={projectActive}
                  expanded={projectExpanded}
                  running={projectRunning}
                  sessionCount={projectSessions.length}
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
                      key={`${project.path}-sessions`}
                      className="overflow-hidden"
                      initial={reduceMotion ? false : { height: 0, opacity: 0 }}
                      animate={{ height: 'auto', opacity: 1 }}
                      exit={{ height: 0, opacity: 0 }}
                      transition={{ duration: reduceMotion ? 0 : 0.16, ease: 'easeOut' }}
                    >
                      <div className="ml-4 mt-0.5 grid gap-0.5">
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
        </div>

        {projects.length === 0 ? (
          <p className="px-3 py-4 font-sans text-xs leading-5 text-ink-faint">{t('sidebar.emptyProjects')}</p>
        ) : null}

        {/*
          稿件的侧栏没有"项目"分组标题那一行，所以打开文件夹的入口挪到列表末尾，
          做成一条低对比度的动作行 —— 应用总得有个地方能加项目。
        */}
        <Button
          type="button"
          variant="ghost"
          className="mt-2 h-9 w-full justify-start gap-2 rounded-xl px-3 text-xs text-ink-faint"
          aria-label={t('sidebar.openFolder')}
          title={t('sidebar.openFolder')}
          disabled={isOpeningDirectory}
          onClick={() => void chooseDirectory()}
        >
          <Plus size={14} className="shrink-0" />
          <span className="min-w-0 truncate">{t('sidebar.openFolder')}</span>
        </Button>
      </div>
    </div>
  )
}

interface ProjectItemProps {
  project: OpenedProject
  active: boolean
  expanded: boolean
  /** 该项目下是否有会话正在跑，决定圆点的颜色。 */
  running?: boolean
  sessionCount?: number
  onSelect: () => void
  onRemove: () => void
  onCreateSession: () => void
}

export function ProjectItem({
  project,
  active,
  expanded,
  running = false,
  sessionCount = 0,
  onSelect,
  onRemove,
  onCreateSession,
}: ProjectItemProps) {
  const { t } = useTranslation()
  return (
    <div data-project-row="true" className="group relative rounded-xl transition hover:bg-paper">
      <button
        type="button"
        /*
          右内边距只留给计数本身。悬停时计数淡出、操作按钮浮在同一块地方，
          所以不需要为按钮预留一段常驻的空白 —— 那会把计数顶到行中间去。
        */
        className="flex h-8 w-full items-center gap-3 rounded-lg pl-2 pr-2.5 text-left text-sm font-semibold text-ink transition"
        title={project.path}
        aria-expanded={expanded}
        onClick={onSelect}
      >
        {expanded
          ? <ChevronDown size={11} className="shrink-0 text-ink-faint" />
          : <ChevronRight size={11} className="shrink-0 text-ink-faint" />}
        <span className={`min-w-0 flex-1 truncate ${active ? 'text-ink' : 'text-ink-soft group-hover:text-ink'}`}>
          {project.name}
        </span>
        <span
          className={`shrink-0 tabular-nums transition group-hover:opacity-0 ${
            running
              ? 'rounded-full bg-clay-soft px-2 py-0.5 text-[11px] font-semibold text-clay'
              : 'text-[11px] font-normal text-ink-faint'
          }`}
          title={t('sidebar.projectSessionCount', { count: sessionCount })}
        >
          {sessionCount > 0 ? t('sidebar.projectSessionCount', { count: sessionCount }) : ''}
        </span>
      </button>
      {/*
        项目操作常驻会把计数挤掉，也和稿件里干净的分组标题不符：只在悬停/键盘聚焦时出现。
      */}
      <div className="absolute right-1 top-1/2 flex -translate-y-1/2 items-center gap-0.5 opacity-0 transition group-hover:opacity-100 group-focus-within:opacity-100">
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
