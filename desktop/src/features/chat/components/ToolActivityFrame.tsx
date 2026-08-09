import type { ReactNode } from 'react'
import { AnimatePresence, motion } from 'motion/react'
import { Activity, ChevronRight } from 'lucide-react'
import { useTranslation } from 'react-i18next'

export type ToolActivityTier = 'readonly' | 'write' | 'failure'

interface ToolActivityFrameProps {
  toolCallId: string
  tier: ToolActivityTier
  expanded: boolean
  onExpandedChange?: (expanded: boolean) => void
  statusIcon: ReactNode
  summary: ReactNode
  inlineActions?: ReactNode
  showDisclosure?: boolean
  notice?: ReactNode
  onOpenTrace?: (providerToolCallId: string) => void
  detailsMaxHeightClass: string
  dataFileChangeActivity?: string
  children: ReactNode
}

export function Separator() {
  return <span aria-hidden="true" className="shrink-0 text-[10px] text-ink-faint/70">·</span>
}

/**
 * 工具卡片只在这里定义状态图标、摘要、目标、量级、耗时与操作区的排列。
 * 各工具组件只负责把自己的返回文本转换为这些展示字段，避免七类工具逐渐长出不同外壳。
 */
export function ToolActivityFrame({
  toolCallId,
  tier,
  expanded,
  onExpandedChange,
  statusIcon,
  summary,
  inlineActions,
  showDisclosure = true,
  notice,
  onOpenTrace,
  detailsMaxHeightClass,
  dataFileChangeActivity,
  children,
}: ToolActivityFrameProps) {
  const { t } = useTranslation()
  const failure = tier === 'failure'
  const rowTone = failure
    ? 'border-status-danger-border bg-status-danger-soft'
    : tier === 'write'
      ? 'border-status-success-border bg-status-success-soft'
      : 'border-line bg-surface/40'
  const detailsBorder = failure
    ? 'border-status-danger-border'
    : tier === 'write'
      ? 'border-status-success-border'
      : 'border-line/70'
  const summaryContent = (
    <>
      {statusIcon}
      {summary}
      {!failure && showDisclosure ? (
        <ChevronRight
          size={13}
          className={`ml-auto shrink-0 text-ink-faint transition-transform duration-200 ${expanded ? 'rotate-90' : ''}`}
        />
      ) : null}
    </>
  )

  return (
    <div
      data-tool-activity-row={toolCallId}
      data-file-change-activity={dataFileChangeActivity}
      data-tool-tier={tier}
      className={`group/row min-w-0 overflow-hidden rounded-lg border ${rowTone}`}
    >
      <div className="flex min-w-0 items-center">
        {failure ? (
          <div className="flex min-h-8 min-w-0 flex-1 items-center gap-1.5 px-2 text-left">
            {summaryContent}
          </div>
        ) : (
          <button
            type="button"
            aria-expanded={expanded}
            onClick={() => onExpandedChange?.(!expanded)}
            className="flex min-h-8 min-w-0 flex-1 items-center gap-1.5 px-2 text-left"
          >
            {summaryContent}
          </button>
        )}
        {inlineActions}
        {onOpenTrace ? (
          <button
            type="button"
            data-open-tool-trace={toolCallId}
            aria-label={t('activity.openToolSpan')}
            title={t('activity.openToolSpan')}
            onClick={() => onOpenTrace(toolCallId)}
            className="mr-1 grid size-6 shrink-0 place-items-center rounded-md text-ink-faint opacity-0 transition-opacity hover:bg-paper hover:text-clay focus-visible:opacity-100 group-hover/row:opacity-100"
          >
            <Activity size={12} />
          </button>
        ) : null}
      </div>

      {notice}

      <AnimatePresence initial={false}>
        {expanded ? (
          <motion.div
            key="details"
            initial={{ opacity: 0, height: 0 }}
            animate={{ opacity: 1, height: 'auto' }}
            exit={{ opacity: 0, height: 0 }}
            transition={{ duration: 0.16, ease: 'easeOut' }}
            className={`overflow-hidden border-t ${detailsBorder}`}
          >
            <div className={`${detailsMaxHeightClass} overflow-auto`}>
              {children}
            </div>
          </motion.div>
        ) : null}
      </AnimatePresence>
    </div>
  )
}
