import { PanelLeftOpen } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { isMacOS } from '@/lib/platform'

/**
 * 顶栏左侧内边距。
 * 侧栏收起后窗口左上角就露出了 macOS 的红绿灯，顶栏必须让开这一块，
 * 否则标题会压在窗口按钮上。每一种占据顶栏的视图都要用这个函数，不要各写各的。
 */
export function headerInsetClass(sidebarExpanded: boolean, macOS: boolean = isMacOS): string {
  return !sidebarExpanded && macOS ? 'pl-20 pr-4' : 'px-4'
}

/**
 * 侧栏收起时的展开入口。放在顶栏最左侧 —— 没有它，收起侧栏后就只能靠快捷键找回来。
 */
export function SidebarReveal({
  sidebarExpanded,
  onToggleSidebar,
}: {
  sidebarExpanded: boolean
  onToggleSidebar: () => void
}) {
  const { t } = useTranslation()
  if (sidebarExpanded) return null
  return (
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
      <div aria-hidden="true" className="h-6 w-px shrink-0 bg-line" />
    </>
  )
}
