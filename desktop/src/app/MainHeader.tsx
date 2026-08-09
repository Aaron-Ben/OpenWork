import { Activity, PanelRightOpen, Settings } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { headerInsetClass, SidebarReveal } from './SidebarReveal'

interface MainHeaderProps {
  title: string | null
  kind?: 'session' | 'activity' | 'settings'
  /** 会话所属项目名，显示在标题前作为面包屑的第一段。 */
  projectName?: string | null
  /** 「主控 + 3 个子智能体 · 12 步 · 01:48」这类运行概况。 */
  subtitle?: string | null
  sidebarExpanded: boolean
  onToggleSidebar: () => void
  onRevealAgentRail?: () => void
}

/**
 * 顶栏只负责"这是哪个会话、跑成什么样"。
 * 右栏收起后把恢复入口留在这里，否则用户无法主动找回智能体面板。
 */
export function MainHeader({
  title,
  kind = 'session',
  projectName,
  subtitle,
  sidebarExpanded,
  onToggleSidebar,
  onRevealAgentRail,
}: MainHeaderProps) {
  const { t } = useTranslation()
  const resolvedTitle = title?.trim() || t('sidebar.untitledSession')
  return (
    <header
      data-main-header="true"
      data-tauri-drag-region="deep"
      className={`flex h-14 shrink-0 items-center gap-3 border-b border-line bg-paper ${headerInsetClass(sidebarExpanded)}`}
    >
      <SidebarReveal sidebarExpanded={sidebarExpanded} onToggleSidebar={onToggleSidebar} />
      {kind === 'activity' ? <Activity size={19} className="shrink-0 text-ink-soft" /> : null}
      {kind === 'settings' ? <Settings size={19} className="shrink-0 text-ink-soft" /> : null}
      <div className="min-w-0 flex-1">
        <h1 className="flex min-w-0 items-baseline gap-1.5 font-sans text-[15px]">
          {kind === 'session' && projectName ? (
            <>
              <span className="max-w-[40%] shrink-0 truncate text-ink-faint">{projectName}</span>
              <span aria-hidden="true" className="shrink-0 text-ink-faint">/</span>
            </>
          ) : null}
          <span className="min-w-0 truncate font-semibold text-ink">{resolvedTitle}</span>
        </h1>
        {subtitle ? (
          <p data-main-header-subtitle="true" className="truncate text-xs text-ink-faint">
            {subtitle}
          </p>
        ) : null}
      </div>
      {onRevealAgentRail ? (
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="size-9 shrink-0 rounded-xl"
          aria-label={t('chat.agents.expand')}
          aria-expanded="false"
          title={t('chat.agents.expand')}
          onClick={onRevealAgentRail}
        >
          <PanelRightOpen size={19} />
        </Button>
      ) : null}
    </header>
  )
}
