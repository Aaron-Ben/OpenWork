import { Activity } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import type { TurnTraceSummary } from '../../type/trace'
import { formatTraceSummary } from './traceViewModel'

export function TurnTraceSummaryButton({
  summary,
  onOpen,
}: {
  summary: TurnTraceSummary
  onOpen: () => void
}) {
  const { i18n, t } = useTranslation()
  return (
    <button
      type="button"
      data-turn-trace-summary={summary.turnId}
      onClick={onOpen}
      className="inline-flex max-w-full items-center gap-1.5 rounded-lg px-1.5 py-1 text-left font-sans text-xs text-ink-faint transition-colors hover:bg-paper-hover hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-clay/30"
      aria-label={t('trace.openDetail')}
    >
      <Activity size={14} aria-hidden="true" />
      <span className="truncate">{formatTraceSummary(summary, i18n.language)}</span>
    </button>
  )
}
