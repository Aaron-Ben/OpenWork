import { useTranslation } from 'react-i18next'
import { Check, Circle, CircleDot } from 'lucide-react'

import type { RuntimePlanStep, RuntimePlanStepStatus } from '@/bridge/compat'

interface PlanCardProps {
  explanation: string | null
  steps: RuntimePlanStep[]
}

interface StatusPresentation {
  icon: typeof Check
  /** 图标颜色只是强化，语义由 statusLabel 的文本承担。 */
  tone: string
  text: string
}

function statusPresentation(status: RuntimePlanStepStatus): StatusPresentation {
  switch (status) {
    case 'completed':
      return { icon: Check, tone: 'text-status-success-ink', text: 'line-through text-ink-faint' }
    case 'in_progress':
      return { icon: CircleDot, tone: 'text-status-warning-ink', text: 'text-ink font-medium' }
    case 'pending':
      return { icon: Circle, tone: 'text-ink-faint', text: 'text-ink-muted' }
  }
}

/**
 * 当前 Turn 的任务清单。
 *
 * 只渲染服务端给的快照：不推进状态、不从助手文本猜进度、不重排步骤。计划的真相在
 * `turn_plans`，这里任何"聪明"的补全都会和它漂移。
 */
export function PlanCard({ explanation, steps }: PlanCardProps) {
  const { t } = useTranslation()

  // 空计划意味着模型显式清空了清单，此时不留占位卡片。
  if (steps.length === 0) return null

  const completed = steps.filter((step) => step.status === 'completed').length

  return (
    <section
      className="rounded-lg border border-line bg-surface-raised px-3 py-2.5 text-sm"
      aria-label={t('chat.plan.title')}
    >
      <header className="mb-2 flex items-baseline justify-between gap-2">
        <h3 className="text-xs font-medium uppercase tracking-wide text-ink-faint">
          {t('chat.plan.title')}
        </h3>
        <span className="text-xs tabular-nums text-ink-faint">
          {t('chat.plan.progress', { completed, total: steps.length })}
        </span>
      </header>

      {explanation ? <p className="mb-2 text-xs text-ink-muted">{explanation}</p> : null}

      <ol className="space-y-1">
        {steps.map((step, index) => {
          const presentation = statusPresentation(step.status)
          const Icon = presentation.icon
          return (
            <li
              // 步骤没有稳定 id，且每次都是整体替换，位置即身份。
              key={`${index}-${step.step}`}
              className="flex items-start gap-2"
            >
              <Icon
                className={`mt-0.5 size-3.5 shrink-0 ${presentation.tone}`}
                aria-hidden="true"
              />
              {/* 状态不能只靠颜色和图标传达。 */}
              <span className="sr-only">{t(`chat.plan.status.${step.status}`)}</span>
              <span className={`min-w-0 break-words ${presentation.text}`}>{step.step}</span>
            </li>
          )
        })}
      </ol>
    </section>
  )
}
