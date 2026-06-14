import { ChevronLeft, ChevronRight, MessageSquare, Settings as SettingsIcon } from 'lucide-react'

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
      className={`grid h-screen overflow-hidden border-r border-stone-200 bg-[#f7f5f0] transition-[grid-template-columns] duration-200 ${
        expanded ? 'grid-cols-[90px_230px]' : 'grid-cols-[90px]'
      } max-[760px]:min-h-[90px] max-[760px]:grid-cols-[90px_1fr_auto] max-[760px]:border-r-0 max-[760px]:border-b`}
    >
      <div className="grid min-h-0 grid-rows-[1fr_auto] border-r border-stone-200">
        <nav className="flex flex-col items-center gap-8 pt-5 max-[760px]:flex-row max-[760px]:justify-center max-[760px]:pt-0">
          <RailButton active={expanded} label={expanded ? 'Collapse sidebar' : 'Expand sidebar'} onClick={onToggleExpanded}>
            {expanded ? <ChevronLeft size={24} /> : <ChevronRight size={24} />}
          </RailButton>
        </nav>

        <div className="flex flex-col items-center border-t border-stone-200 py-4 max-[760px]:flex-row max-[760px]:border-t-0 max-[760px]:border-l max-[760px]:px-4 max-[760px]:py-0">
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
    <div className="flex min-w-0 flex-col border-r border-stone-200 bg-[#fbfaf7] max-[760px]:hidden">
      <div className="min-h-0 flex-1 overflow-auto px-3 py-4">
        <div className="mb-3 px-2 text-xs font-medium uppercase tracking-wide text-stone-400">Sessions</div>
        <button type="button" className="flex w-full items-center gap-3 rounded-lg bg-white px-3 py-2.5 text-left text-sm font-medium text-slate-900 shadow-sm">
          <MessageSquare size={17} className="shrink-0 text-orange-700" />
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
    <button type="button" className="flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-left text-sm text-stone-600 hover:bg-white hover:text-slate-900">
      <MessageSquare size={16} className="shrink-0" />
      <span className="min-w-0 flex-1 truncate">{title}</span>
    </button>
  )
}

function RailButton({ active, label, children, onClick }: { active: boolean; label: string; children: React.ReactNode; onClick?: () => void }) {
  return (
    <button
      className={`grid size-11 place-items-center rounded-xl text-stone-600 transition ${
        active ? 'bg-white text-orange-700 shadow-[0_0_22px_rgba(124,58,237,0.25)]' : 'hover:bg-white hover:text-slate-900'
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
