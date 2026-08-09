import { useEffect, useRef, useState } from 'react'
import { AnimatePresence, motion } from 'motion/react'
import { Check, ChevronDown, Circle, ListChecks } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import type { RuntimePlanStep, RuntimePlanStepStatus } from '@/bridge/compat'
import type { TurnPlanView } from '@/types/chat'

interface PlanCardProps {
  plan: TurnPlanView
}

interface StatusPresentation {
  icon: typeof Check
  iconClass: string
  rowClass: string
  textClass: string
}

function statusPresentation(status: RuntimePlanStepStatus): StatusPresentation {
  switch (status) {
    case 'completed':
      return {
        icon: Check,
        iconClass: 'bg-status-success text-paper',
        rowClass: '',
        textClass: 'line-through text-ink-faint',
      }
    case 'in_progress':
      return {
        icon: Circle,
        iconClass: 'fill-status-success text-status-success',
        rowClass: 'border-l-[3px] border-status-success bg-status-success-soft',
        textClass: 'font-semibold text-ink',
      }
    case 'pending':
      return {
        icon: Circle,
        iconClass: 'text-ink-faint',
        rowClass: '',
        textClass: 'text-ink-muted',
      }
  }
}

function stepKeys(steps: RuntimePlanStep[]): string[] {
  const occurrences = new Map<string, number>()
  return steps.map((step) => {
    const occurrence = occurrences.get(step.step) ?? 0
    occurrences.set(step.step, occurrence + 1)
    return `${step.step}\u0000${occurrence}`
  })
}

function elapsedMs(startedAt: string | null, updatedAt: string): number | null {
  if (!startedAt) return null
  const start = Date.parse(startedAt)
  const end = Date.parse(updatedAt)
  if (!Number.isFinite(start) || !Number.isFinite(end) || end < start) return null
  return end - start
}

/**
 * update_plan 是会话状态，不是一组需要阅读输入输出的工具记录。
 * 组件只比较相邻快照，不自行推进步骤，避免和 Core 的计划真相漂移。
 */
export function PlanCard({ plan }: PlanCardProps) {
  const { t } = useTranslation()
  const completed = plan.steps.filter((step) => step.status === 'completed').length
  const allCompleted = completed === plan.steps.length && plan.steps.length > 0
  const collapseCompletedSteps = plan.steps.length > 12 && !allCompleted
  const [expanded, setExpanded] = useState(() => !allCompleted)
  const [showCompletedSteps, setShowCompletedSteps] = useState(false)
  const [flashKeys, setFlashKeys] = useState<ReadonlySet<string>>(new Set())
  const previousStepsRef = useRef(plan.steps)
  const flashTimerRef = useRef<number | null>(null)

  useEffect(() => {
    setExpanded(!allCompleted)
  }, [allCompleted])

  useEffect(() => {
    const previous = previousStepsRef.current
    const previousByKey = new Map(
      stepKeys(previous).map((key, index) => [key, previous[index]] as const),
    )
    const changed = new Set(
      stepKeys(plan.steps).filter((key, index) => {
        const oldStep = previousByKey.get(key)
        return oldStep != null && oldStep.status !== plan.steps[index].status
      }),
    )
    previousStepsRef.current = plan.steps
    if (changed.size === 0) return

    setFlashKeys(changed)
    if (flashTimerRef.current != null) window.clearTimeout(flashTimerRef.current)
    flashTimerRef.current = window.setTimeout(() => setFlashKeys(new Set()), 600)
    return () => {
      if (flashTimerRef.current != null) window.clearTimeout(flashTimerRef.current)
    }
  }, [plan.steps, plan.updatedAt])

  if (plan.steps.length === 0) return null

  const duration = elapsedMs(plan.startedAt, plan.updatedAt)
  const totalSeconds = duration == null ? null : Math.max(0, Math.round(duration / 1000))
  const durationText = totalSeconds == null
    ? null
    : totalSeconds >= 60
      ? t('chat.plan.minutesSeconds', {
          minutes: Math.floor(totalSeconds / 60),
          seconds: totalSeconds % 60,
        })
      : t('chat.plan.seconds', { seconds: totalSeconds })
  const completedSummary = [
    t('chat.plan.completed', { count: plan.steps.length }),
    durationText == null ? null : t('chat.plan.duration', { duration: durationText }),
  ].filter(Boolean).join(' · ')
  const keys = stepKeys(plan.steps)
  const visibleSteps = plan.steps.flatMap((step, index) => (
    collapseCompletedSteps && !showCompletedSteps && step.status === 'completed'
      ? []
      : [{ step, key: keys[index] }]
  ))

  return (
    <section
      data-plan-card="true"
      className="overflow-hidden rounded-xl border border-line bg-surface-raised text-sm"
      aria-label={t('chat.plan.title')}
    >
      {allCompleted ? (
        <button
          type="button"
          aria-expanded={expanded}
          onClick={() => setExpanded((value) => !value)}
          className="flex min-h-11 w-full min-w-0 items-center gap-2 px-3 text-left hover:bg-paper-hover"
        >
          <span className="grid size-5 shrink-0 place-items-center rounded-full bg-status-success text-paper">
            <Check size={13} aria-hidden="true" />
          </span>
          <span className="min-w-0 flex-1 truncate font-medium text-ink" title={completedSummary}>
            {completedSummary}
          </span>
          <ChevronDown
            size={14}
            className={`shrink-0 text-ink-faint transition-transform ${expanded ? 'rotate-180' : ''}`}
            aria-hidden="true"
          />
        </button>
      ) : (
        <header className="flex min-h-12 items-center gap-2.5 border-b border-line px-3">
          <ListChecks size={16} className="shrink-0 text-status-success-ink" aria-hidden="true" />
          <h3 className="shrink-0 text-sm font-semibold text-ink">{t('chat.plan.title')}</h3>
          <div
            className="h-1.5 min-w-12 flex-1 overflow-hidden rounded-full bg-line"
            role="progressbar"
            aria-valuemin={0}
            aria-valuemax={plan.steps.length}
            aria-valuenow={completed}
          >
            <div
              className="h-full rounded-full bg-status-success transition-[width] duration-300"
              style={{ width: `${(completed / plan.steps.length) * 100}%` }}
            />
          </div>
          <span className="shrink-0 text-xs tabular-nums text-ink-muted">
            {t('chat.plan.progress', { completed, total: plan.steps.length })}
          </span>
          <span className="shrink-0 text-xs text-ink-faint">
            {t('chat.plan.updateCount', { count: plan.updateCount })}
          </span>
        </header>
      )}

      <AnimatePresence initial={false}>
        {expanded ? (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: 'auto', opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ duration: 0.18 }}
          >
            {plan.explanation ? (
              <p className="border-b border-line px-3 py-2 text-xs text-ink-muted">
                {plan.explanation}
              </p>
            ) : null}
            {collapseCompletedSteps ? (
              <button
                type="button"
                aria-expanded={showCompletedSteps}
                onClick={() => setShowCompletedSteps((value) => !value)}
                className="mx-3 mt-2 inline-flex items-center gap-1 text-xs text-ink-muted hover:text-ink"
              >
                {t('chat.plan.completedSteps', { count: completed })}
                <ChevronDown
                  size={12}
                  className={`transition-transform ${showCompletedSteps ? 'rotate-180' : ''}`}
                  aria-hidden="true"
                />
              </button>
            ) : null}
            <ol className="space-y-1 p-2">
              <AnimatePresence initial={false} mode="popLayout">
                {visibleSteps.map(({ step, key }) => {
                  const presentation = statusPresentation(step.status)
                  const Icon = presentation.icon
                  return (
                    <motion.li
                      layout
                      key={key}
                      initial={{ opacity: 0, height: 0 }}
                      animate={{ opacity: 1, height: 'auto' }}
                      exit={{ opacity: 0, height: 0 }}
                      transition={{ duration: 0.18 }}
                      className={`flex min-h-8 min-w-0 items-center gap-2 rounded-md px-2 ${presentation.rowClass}${
                        flashKeys.has(key) ? ' plan-step-flash' : ''
                      }`}
                    >
                      <span className={`grid size-4 shrink-0 place-items-center rounded-full ${presentation.iconClass}`}>
                        <Icon size={step.status === 'completed' ? 11 : 14} aria-hidden="true" />
                      </span>
                      <span className="sr-only">{t(`chat.plan.status.${step.status}`)}</span>
                      <span className={`min-w-0 flex-1 truncate ${presentation.textClass}`} title={step.step}>
                        {step.step}
                      </span>
                      {step.status === 'in_progress' ? (
                        <span className="shrink-0 rounded-full bg-paper/70 px-2 py-0.5 text-[11px] font-medium text-status-success-ink">
                          {t('chat.plan.status.in_progress')}
                        </span>
                      ) : null}
                    </motion.li>
                  )
                })}
              </AnimatePresence>
            </ol>
          </motion.div>
        ) : null}
      </AnimatePresence>
    </section>
  )
}
