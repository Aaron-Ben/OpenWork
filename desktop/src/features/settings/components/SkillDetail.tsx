import { ArrowLeft } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { coreCommands } from '@/bridge/commands'
import type { RuntimeSkillDetail, RuntimeSkillSummary } from '@/bridge/compat'
import { MarkdownRenderer } from '@/components/markdown/MarkdownRenderer'
import { resolveErrorMessage } from '@/lib/commandError'

import { displaySkillPath } from './SkillList'

export function displaySkillName(name: string): string {
  return name
    .split('-')
    .filter(Boolean)
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(' ')
}

interface SkillDetailProps {
  skill: RuntimeSkillSummary
  onBack: () => void
}

export function SkillDetail({ skill, onBack }: SkillDetailProps) {
  const [detail, setDetail] = useState<RuntimeSkillDetail | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const requestSequence = useRef(0)

  useEffect(() => {
    const request = ++requestSequence.current
    setLoading(true)
    setError(null)
    coreCommands
      .readSkill(skill.path)
      .then((next) => {
        if (request === requestSequence.current) setDetail(next)
      })
      .catch((reason: unknown) => {
        if (request === requestSequence.current) setError(resolveErrorMessage(reason))
      })
      .finally(() => {
        if (request === requestSequence.current) setLoading(false)
      })
    return () => {
      requestSequence.current += 1
    }
  }, [skill.path])

  return (
    <SkillDetailView
      skill={skill}
      detail={detail}
      loading={loading}
      error={error}
      onBack={onBack}
    />
  )
}

interface SkillDetailViewProps {
  skill: RuntimeSkillSummary
  detail: RuntimeSkillDetail | null
  loading: boolean
  error: string | null
  onBack: () => void
}

export function SkillDetailView({ skill, detail, loading, error, onBack }: SkillDetailViewProps) {
  const { t } = useTranslation()

  return (
    <section>
      <button
        type="button"
        onClick={onBack}
        className="inline-flex h-8 items-center gap-2 rounded-lg border border-line px-3 font-sans text-xs font-medium text-ink-soft transition hover:bg-paper-hover"
      >
        <ArrowLeft size={14} />
        {t('settings.skills.detail.back')}
      </button>
      <header className="mt-6">
        <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
          <h2 className="font-sans text-2xl font-semibold text-ink">
            {displaySkillName(skill.name)}
          </h2>
          <span className="rounded-md border border-line px-1.5 py-0.5 font-sans text-xs text-ink-faint">
            {t('settings.skills.detail.badge')}
          </span>
        </div>
        <p className="mt-2 font-sans text-sm leading-6 text-ink-soft">{skill.description}</p>
        <p
          className="mt-2 break-all font-mono text-[11px] leading-5 text-ink-faint"
          title={skill.path}
        >
          {displaySkillPath(skill.path)}
        </p>
      </header>
      <div className="mt-6 rounded-2xl border border-line bg-paper px-6 py-5 max-[640px]:px-4">
        {loading ? (
          <p className="font-sans text-sm text-ink-faint">{t('settings.skills.detail.loading')}</p>
        ) : error ? (
          <p className="font-sans text-sm text-status-danger-ink" role="alert">
            {t('settings.skills.detail.loadFailed', { reason: error })}
          </p>
        ) : detail ? (
          <MarkdownRenderer content={detail.body} variant="document" />
        ) : null}
      </div>
    </section>
  )
}
