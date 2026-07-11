import { useState } from 'react'
import {
  Check,
  ChevronLeft,
  ChevronRight,
  MessageSquare,
  Monitor,
  Moon,
  Pencil,
  Plus,
  Settings as SettingsIcon,
  Sun,
  Trash2,
  X,
} from 'lucide-react'

import { useActiveProvider } from '../../stores/providerStore'
import { useSessionStore } from '../../stores/sessionStore'
import { useThemeStore, type Theme } from '../../stores/themeStore'
import type { SessionSummary } from '../../type/session'
import type { AppView } from './types'

interface SidebarProps {
  view: AppView
  expanded: boolean
  onToggleExpanded: () => void
  onNavigate: (next: AppView) => void
}

export function Sidebar({ view, expanded, onToggleExpanded, onNavigate }: SidebarProps) {
  return (
    <aside
      className={`grid h-screen overflow-hidden border-r border-line bg-paper-hover transition-[grid-template-columns] duration-200 ${
        expanded ? 'grid-cols-[90px_230px]' : 'grid-cols-[90px]'
      } max-[760px]:min-h-[90px] max-[760px]:grid-cols-[90px_1fr_auto] max-[760px]:border-r-0 max-[760px]:border-b`}
    >
      <div className="grid min-h-0 grid-rows-[1fr_auto] border-r border-line">
        <nav className="flex flex-col items-center gap-8 pt-5 max-[760px]:flex-row max-[760px]:justify-center max-[760px]:pt-0">
          <RailButton active={expanded} label={expanded ? 'Collapse sidebar' : 'Expand sidebar'} onClick={onToggleExpanded}>
            {expanded ? <ChevronLeft size={24} /> : <ChevronRight size={24} />}
          </RailButton>
        </nav>

        <div className="flex flex-col items-center gap-2 border-t border-line py-4 max-[760px]:flex-row max-[760px]:border-t-0 max-[760px]:border-l max-[760px]:px-4 max-[760px]:py-0">
          <ThemeToggle />
          <RailButton active={view === 'settings'} label="Settings" onClick={() => onNavigate('settings')}>
            <SettingsIcon size={25} />
          </RailButton>
        </div>
      </div>

      {expanded ? <SidebarPanel /> : null}
    </aside>
  )
}

function SidebarPanel() {
  const sessions = useSessionStore((state) => state.sessions)
  const activeSessionId = useSessionStore((state) => state.activeSessionId)
  const select = useSessionStore((state) => state.select)
  const create = useSessionStore((state) => state.create)
  const rename = useSessionStore((state) => state.rename)
  const remove = useSessionStore((state) => state.remove)
  const active = useActiveProvider()

  async function handleCreate() {
    if (!active) return
    await create({
      providerId: active.id,
      model: active.models.find((item) => item.enabled)?.modelId ?? '',
      title: 'New session',
    })
  }

  return (
    <div className="flex min-w-0 flex-col border-r border-line bg-paper max-[760px]:hidden">
      <div className="px-3 py-3">
        <button
          type="button"
          onClick={() => void handleCreate()}
          disabled={!active}
          className="flex w-full items-center justify-center gap-2 rounded-lg bg-clay px-3 py-2.5 text-sm font-semibold text-white transition hover:bg-clay/90 disabled:cursor-not-allowed disabled:bg-paper-hover disabled:text-ink-faint"
        >
          <Plus size={16} />
          新建会话
        </button>
      </div>
      <div className="min-h-0 flex-1 overflow-auto px-3 pb-4">
        <div className="mb-2 px-2 text-xs font-medium uppercase tracking-wide text-ink-faint">Sessions</div>
        <div className="grid gap-1">
          {sessions.map((session) => (
            <SessionItem
              key={session.id}
              session={session}
              active={activeSessionId === session.id}
              onSelect={() => void select(session.id)}
              onRename={(title) => void rename(session.id, title)}
              onDelete={() => void remove(session.id)}
            />
          ))}
          {sessions.length === 0 ? (
            <div className="px-2 py-4 text-xs text-ink-faint">暂无会话</div>
          ) : null}
        </div>
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

/// 单个会话项:正常态(悬停显示重命名/删除)、编辑态(就地输入框)、删除二次确认态。
function SessionItem({ session, active, onSelect, onRename, onDelete }: SessionItemProps) {
  const [mode, setMode] = useState<ItemMode>('view')
  const [draft, setDraft] = useState(session.title)

  function startEdit() {
    setDraft(session.title)
    setMode('edit')
  }

  function commitRename() {
    const title = draft.trim()
    if (title && title !== session.title) {
      onRename(title)
    } else {
      setDraft(session.title)
    }
    setMode('view')
  }

  if (mode === 'edit') {
    return (
      <form
        onSubmit={(event) => {
          event.preventDefault()
          commitRename()
        }}
        className="flex items-center gap-1 rounded-lg bg-paper-hover px-2 py-1.5"
      >
        <input
          autoFocus
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          onBlur={commitRename}
          onKeyDown={(event) => {
            if (event.key === 'Escape') {
              setDraft(session.title)
              setMode('view')
            }
          }}
          className="min-w-0 flex-1 rounded border border-line bg-paper px-2 py-1 text-sm text-ink outline-none focus:border-clay"
        />
        <button
          type="submit"
          className="grid size-6 shrink-0 place-items-center rounded text-emerald-600 hover:bg-paper"
          title="确认"
        >
          <Check size={13} />
        </button>
      </form>
    )
  }

  if (mode === 'confirm-delete') {
    return (
      <div className="flex items-center gap-1 rounded-lg bg-rose-50 px-2 py-2">
        <span className="min-w-0 flex-1 truncate text-xs text-rose-700">删除该会话?</span>
        <button
          type="button"
          onClick={() => {
            onDelete()
            setMode('view')
          }}
          className="grid size-6 shrink-0 place-items-center rounded text-rose-600 hover:bg-paper"
          title="确认删除"
        >
          <Check size={13} />
        </button>
        <button
          type="button"
          onClick={() => setMode('view')}
          className="grid size-6 shrink-0 place-items-center rounded text-ink-faint hover:bg-paper hover:text-ink"
          title="取消"
        >
          <X size={13} />
        </button>
      </div>
    )
  }

  return (
    <div className="group relative">
      <button
        type="button"
        onClick={onSelect}
        className={`flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-left text-sm transition ${
          active
            ? 'bg-paper-hover font-medium text-ink shadow-sm'
            : 'text-ink-soft hover:bg-paper-hover hover:text-ink'
        }`}
      >
        <MessageSquare
          size={16}
          className={`shrink-0 ${active ? 'text-clay' : 'text-ink-faint'}`}
        />
        <span className="min-w-0 flex-1 truncate pr-12">{session.title}</span>
      </button>
      <div className="absolute right-1 top-1/2 flex -translate-y-1/2 items-center gap-0.5 opacity-0 transition group-hover:opacity-100">
        <button
          type="button"
          onClick={startEdit}
          className="grid size-6 place-items-center rounded text-ink-faint hover:bg-paper hover:text-ink"
          title="重命名"
        >
          <Pencil size={12} />
        </button>
        <button
          type="button"
          onClick={() => setMode('confirm-delete')}
          className="grid size-6 place-items-center rounded text-ink-faint hover:bg-paper hover:text-rose-500"
          title="删除"
        >
          <Trash2 size={12} />
        </button>
      </div>
    </div>
  )
}

/// 亮/暗/跟随系统三态循环切换;图标随当前偏好变化。
function ThemeToggle() {
  const theme = useThemeStore((state) => state.theme)
  const cycleTheme = useThemeStore((state) => state.cycleTheme)
  const Icon = theme === 'light' ? Sun : theme === 'dark' ? Moon : Monitor
  const label: Record<Theme, string> = { light: '亮色', dark: '暗色', system: '跟随系统' }

  return (
    <RailButton active={false} label={`主题：${label[theme]}（点击切换）`} onClick={cycleTheme}>
      <Icon size={24} />
    </RailButton>
  )
}

function RailButton({ active, label, children, onClick }: { active: boolean; label: string; children: React.ReactNode; onClick?: () => void }) {
  return (
    <button
      className={`grid size-11 place-items-center rounded-xl text-ink-soft transition ${
        active
          ? 'bg-paper text-clay shadow-[0_0_22px_rgba(217,119,87,0.28)]'
          : 'hover:bg-paper hover:text-ink'
      }`}
      type="button"
      onClick={onClick}
      aria-label={label}
      title={label}
    >
      {children}
    </button>
  )
}
