import { useTranslation } from 'react-i18next'

import type { RuntimeTraceSpan } from '@/bridge/compat'
import { cn } from '@/lib/utils'
import { permissionCategory, type PermissionCategory } from '../permissionCategory'

/** 配色按 D7 原型：用户批准的两类实心，拒绝与未执行描边或虚线，其余浅底。 */
const STYLES: Record<PermissionCategory, string> = {
  sandbox_auto: 'bg-status-success-soft text-status-success-ink',
  sandbox_denied: 'bg-status-warning-soft text-status-warning-ink',
  user_approved_escalation: 'bg-clay text-paper',
  user_approved_danger: 'bg-status-danger text-paper',
  sandbox_unavailable: 'border border-dashed border-status-warning-border text-status-warning-ink',
  rule_denied: 'border border-dashed border-line-strong bg-paper-hover text-ink-soft',
  user_denied: 'border border-status-danger-border text-status-danger-ink',
  control_tool: 'bg-paper-hover text-ink-soft',
  cancelled: 'bg-paper-hover text-ink-soft',
  unknown: 'bg-paper-hover text-ink-faint',
}

/** Tool Span 的权限类别（permissions.md §14.1）；其他 Span 不渲染。 */
export function PermissionCategoryBadge({ span, className }: { span: RuntimeTraceSpan; className?: string }) {
  const { t } = useTranslation()
  const category = permissionCategory(span)
  if (!category) return null
  return (
    <span
      data-permission-category={category}
      className={cn(
        'inline-flex max-w-full shrink-0 items-center rounded-full px-2 py-0.5 text-[10px] font-medium leading-none',
        STYLES[category],
        className,
      )}
    >
      <span className="truncate">{t(`activity.permissionCategory.${category}`)}</span>
    </span>
  )
}
