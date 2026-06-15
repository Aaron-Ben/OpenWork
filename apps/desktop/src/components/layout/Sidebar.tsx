import {
  ChevronLeft,
  ChevronRight,
  MessageSquare,
  Monitor,
  Moon,
  Settings as SettingsIcon,
  Sun,
} from 'lucide-react'

import { useThemeStore, type Theme } from '../../stores/themeStore'
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
  return (
    <div className="flex min-w-0 flex-col border-r border-line bg-paper max-[760px]:hidden">
      <div className="min-h-0 flex-1 overflow-auto px-3 py-4">
        <div className="mb-3 px-2 text-xs font-medium uppercase tracking-wide text-ink-faint">Sessions</div>
        <button type="button" className="flex w-full items-center gap-3 rounded-lg bg-paper-hover px-3 py-2.5 text-left text-sm font-medium text-ink shadow-sm">
          <MessageSquare size={17} className="shrink-0 text-clay" />
          <span className="min-w-0 flex-1 truncate">Untitled Session</span>
        </button>
        <div className="mt-2 grid gap-1">
          <SessionPlaceholder title="分析下最近的commit" />
        </div>
      </div>
    </div>
  )
}

function SessionPlaceholder({ title }: { title: string }) {
  return (
    <button type="button" className="flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-left text-sm text-ink-soft hover:bg-paper-hover hover:text-ink">
      <MessageSquare size={16} className="shrink-0" />
      <span className="min-w-0 flex-1 truncate">{title}</span>
    </button>
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
