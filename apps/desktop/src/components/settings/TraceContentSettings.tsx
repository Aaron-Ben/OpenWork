import { FileWarning } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { RuntimeTraceContentPolicy } from '../../bridge/compat'
import { useTraceContentStore } from '../../stores/traceContentStore'

const POLICIES: RuntimeTraceContentPolicy[] = ['full', 'compaction_only', 'off']

export function TraceContentSettings() {
  const { t } = useTranslation()
  const policy = useTraceContentStore((state) => state.policy)
  const updating = useTraceContentStore((state) => state.updating)
  const error = useTraceContentStore((state) => state.error)
  const updatePolicy = useTraceContentStore((state) => state.updatePolicy)
  const [draft, setDraft] = useState<RuntimeTraceContentPolicy>(policy)
  const [saved, setSaved] = useState(false)

  useEffect(() => setDraft(policy), [policy])

  return (
    <div className="mx-auto w-full max-w-4xl p-8 max-[640px]:p-5">
      <div className="mb-7">
        <h2 className="font-sans text-2xl font-semibold text-ink">
          {t('settings.traceContent.title')}
        </h2>
        <p className="mt-2 font-sans text-sm text-ink-faint">
          {t('settings.traceContent.description')}
        </p>
      </div>

      <form
        className="max-w-xl rounded-2xl border border-line bg-paper p-5"
        onSubmit={(event) => {
          event.preventDefault()
          setSaved(false)
          void updatePolicy(draft).then(setSaved)
        }}
      >
        <div className="flex items-start gap-3">
          <FileWarning size={21} className="mt-0.5 shrink-0 text-status-warning-ink" />
          <div className="min-w-0 flex-1">
            <label htmlFor="trace-content-policy" className="text-sm font-semibold text-ink">
              {t('settings.traceContent.policyLabel')}
            </label>
            <p className="mt-1 text-xs leading-5 text-ink-faint">
              {t('settings.traceContent.sourceWarning')}
            </p>
            <select
              id="trace-content-policy"
              value={draft}
              disabled={updating}
              className="mt-4 h-10 w-full rounded-lg border border-line-strong bg-paper px-3 text-sm text-ink outline-none focus:ring-3 focus:ring-clay/20"
              onChange={(event) => {
                setDraft(event.target.value as RuntimeTraceContentPolicy)
                setSaved(false)
              }}
            >
              {POLICIES.map((value) => (
                <option key={value} value={value}>
                  {t(`settings.traceContent.policies.${value}.label`)}
                </option>
              ))}
            </select>
            <p className="mt-2 text-xs leading-5 text-ink-faint">
              {t(`settings.traceContent.policies.${draft}.description`)}
            </p>
            <button
              type="submit"
              disabled={updating || draft === policy}
              className="mt-4 h-10 rounded-lg bg-clay px-4 text-sm font-medium text-white transition hover:bg-clay/90 disabled:cursor-not-allowed disabled:opacity-45"
            >
              {updating ? t('settings.traceContent.saving') : t('settings.traceContent.save')}
            </button>
            {error ? (
              <p className="mt-2 text-xs text-status-danger-ink" role="alert">{error}</p>
            ) : saved ? (
              <p className="mt-2 text-xs text-status-success-ink" role="status">
                {t('settings.traceContent.saved')}
              </p>
            ) : null}
          </div>
        </div>
      </form>

      <p className="mt-4 max-w-xl text-xs leading-5 text-ink-faint">
        {t('settings.traceContent.runtimeNote')}
      </p>
    </div>
  )
}
