import { AlertTriangle, ChevronRight, Package, Puzzle, RefreshCw } from 'lucide-react'
import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { coreCommands } from '@/bridge/commands'
import type { RuntimeSkillDiscovery, RuntimeSkillSummary } from '@/bridge/compat'

const SKILLS_ROOT_MARKER = '/.agents/skills/'

export function displaySkillPath(path: string): string {
  const index = path.indexOf(SKILLS_ROOT_MARKER)
  return index > 0 ? `~${path.slice(index)}` : path
}

export function SkillList({ onSelectSkill }: { onSelectSkill: (skill: RuntimeSkillSummary) => void }) {
  const [discovery, setDiscovery] = useState<RuntimeSkillDiscovery | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [updatingNames, setUpdatingNames] = useState<ReadonlySet<string>>(new Set())
  const requestSequence = useRef(0)
  const operationInFlight = useRef(false)

  const refresh = useCallback(async () => {
    if (operationInFlight.current) return
    operationInFlight.current = true
    const request = ++requestSequence.current
    setLoading(true)
    setError(null)
    try {
      const next = await coreCommands.listSkills()
      if (request === requestSequence.current) setDiscovery(next)
    } catch (reason) {
      if (request === requestSequence.current) {
        setError(reason instanceof Error ? reason.message : String(reason))
      }
    } finally {
      if (request === requestSequence.current) setLoading(false)
      operationInFlight.current = false
    }
  }, [])

  const setDisabled = useCallback(async (skill: RuntimeSkillSummary, disabled: boolean) => {
    if (operationInFlight.current) return
    operationInFlight.current = true
    const request = ++requestSequence.current
    setError(null)
    setUpdatingNames((current) => new Set(current).add(skill.name))
    try {
      const next = await coreCommands.setSkillDisabled(skill.name, disabled)
      if (request === requestSequence.current) setDiscovery(next)
    } catch (reason) {
      if (request === requestSequence.current) {
        setError(reason instanceof Error ? reason.message : String(reason))
      }
    } finally {
      setUpdatingNames((current) => {
        const next = new Set(current)
        next.delete(skill.name)
        return next
      })
      operationInFlight.current = false
    }
  }, [])

  useEffect(() => {
    void refresh()
  }, [refresh])

  return (
    <SkillListView
      discovery={discovery}
      loading={loading}
      error={error}
      onRefresh={() => void refresh()}
      onSelectSkill={onSelectSkill}
      onSetDisabled={(skill, disabled) => void setDisabled(skill, disabled)}
      updatingNames={updatingNames}
    />
  )
}

interface SkillListViewProps {
  discovery: RuntimeSkillDiscovery | null
  loading: boolean
  error: string | null
  onRefresh?: () => void
  onSelectSkill?: (skill: RuntimeSkillSummary) => void
  onSetDisabled?: (skill: RuntimeSkillSummary, disabled: boolean) => void
  updatingNames?: ReadonlySet<string>
}

export function SkillListView({
  discovery,
  loading,
  error,
  onRefresh,
  onSelectSkill,
  onSetDisabled,
  updatingNames = new Set(),
}: SkillListViewProps) {
  const { t } = useTranslation()

  return (
    <section className="overflow-hidden rounded-2xl border border-line bg-paper">
      <header className="flex items-center gap-3 border-b border-line px-5 py-4">
        <div className="grid size-9 shrink-0 place-items-center rounded-xl bg-clay-soft text-clay">
          <Puzzle size={18} />
        </div>
        <div className="min-w-0 flex-1">
          <h3 className="font-sans text-sm font-semibold text-ink">{t('settings.skills.title')}</h3>
        </div>
        {onRefresh ? (
          <button
            type="button"
            disabled={loading || updatingNames.size > 0}
            className="inline-flex h-9 shrink-0 items-center gap-2 rounded-lg border border-line px-3 font-sans text-xs font-medium text-ink-soft transition hover:bg-paper-hover disabled:cursor-not-allowed disabled:opacity-50"
            onClick={onRefresh}
          >
            <RefreshCw size={14} className={loading ? 'animate-spin' : undefined} />
            {t('settings.skills.refresh')}
          </button>
        ) : null}
      </header>
      <div className="px-5 py-5">
        {loading && !discovery ? (
          <p className="font-sans text-sm text-ink-faint">{t('settings.skills.loading')}</p>
        ) : error ? (
          <p className="font-sans text-sm text-status-danger-ink" role="alert">
            {t('settings.skills.loadFailed', { reason: error })}
          </p>
        ) : !discovery ? (
          <p className="font-sans text-sm text-ink-faint">{t('settings.skills.loading')}</p>
        ) : discovery.skills.length === 0 && discovery.warnings.length === 0 ? (
          <p className="font-sans text-sm text-ink-faint">{t('settings.skills.empty')}</p>
        ) : (
          <div>
            <div className="grid gap-1">
              {discovery.skills.map((skill) => (
                <article
                  key={`${skill.source}:${skill.path}`}
                  className="flex min-w-0 items-center gap-3 rounded-xl px-3 py-2 transition hover:bg-paper-hover"
                >
                  <button
                    type="button"
                    disabled={!onSelectSkill}
                    onClick={() => onSelectSkill?.(skill)}
                    className="flex min-w-0 flex-1 items-center gap-3 py-1 text-left disabled:cursor-default"
                  >
                    <div className="grid size-9 shrink-0 place-items-center rounded-full border border-line text-ink-faint">
                      <Package size={16} />
                    </div>
                    <div className="min-w-0 flex-1">
                      <h4 className="font-sans text-sm font-semibold text-ink">{skill.name}</h4>
                      <p
                        className="mt-0.5 truncate font-sans text-xs leading-5 text-ink-faint"
                        title={skill.description}
                      >
                        {skill.description}
                      </p>
                    </div>
                    {onSelectSkill ? (
                      <ChevronRight size={16} className="shrink-0 text-ink-faint" />
                    ) : null}
                  </button>
                  <div className="flex shrink-0 items-center gap-3 pl-2">
                    <div className="text-right">
                      <p className="font-sans text-xs font-medium text-ink-soft">
                        {t(skill.disabled ? 'settings.skills.disabled' : 'settings.skills.enabled')}
                      </p>
                      <p className="mt-0.5 font-sans text-[11px] text-ink-faint">
                        {t(
                          skill.disabled
                            ? 'settings.skills.disabledHint'
                            : 'settings.skills.enabledHint',
                        )}
                      </p>
                    </div>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={!skill.disabled}
                      aria-label={t('settings.skills.toggle', { name: skill.name })}
                      disabled={!onSetDisabled || loading || updatingNames.size > 0}
                      onClick={() => onSetDisabled?.(skill, !skill.disabled)}
                      className={`relative h-6 w-11 rounded-full transition disabled:cursor-not-allowed disabled:opacity-50 ${
                        skill.disabled ? 'bg-line-strong' : 'bg-clay'
                      }`}
                    >
                      <span
                        className={`absolute top-0.5 left-0.5 size-5 rounded-full bg-white shadow-sm transition-transform ${
                          skill.disabled ? '' : 'translate-x-5'
                        }`}
                      />
                    </button>
                  </div>
                </article>
              ))}
            </div>
            {discovery.warnings.length > 0 ? (
              <div className="mt-3 grid gap-3">
                {discovery.warnings.map((warning) => (
                  <article
                    key={`${warning.path}:${warning.reason}`}
                    className="rounded-xl border border-status-warning-border bg-status-warning-soft px-4 py-3"
                  >
                    <div className="flex items-start gap-2">
                      <AlertTriangle size={15} className="mt-0.5 shrink-0 text-status-warning-ink" />
                      <div className="min-w-0">
                        <p
                          className="break-all font-mono text-[11px] leading-5 text-ink-faint"
                          title={warning.path}
                        >
                          {displaySkillPath(warning.path)}
                        </p>
                        <p className="mt-1 font-sans text-xs leading-5 text-status-warning-ink">
                          {t('settings.skills.notLoaded', { reason: warning.reason })}
                        </p>
                      </div>
                    </div>
                  </article>
                ))}
              </div>
            ) : null}
          </div>
        )}
      </div>
    </section>
  )
}
