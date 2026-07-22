import { CircleGauge } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  parseContextWindowTokens,
  useContextWindowStore,
} from '../../stores/contextWindowStore'

export function ContextWindowSettings() {
  const { t } = useTranslation()
  const contextWindowTokens = useContextWindowStore((state) => state.contextWindowTokens)
  const setContextWindowTokens = useContextWindowStore((state) => state.setContextWindowTokens)
  const [draft, setDraft] = useState(String(contextWindowTokens))
  const [saved, setSaved] = useState(false)
  const parsed = parseContextWindowTokens(draft)

  useEffect(() => setDraft(String(contextWindowTokens)), [contextWindowTokens])

  return (
    <div className="mx-auto w-full max-w-4xl p-8 max-[640px]:p-5">
      <div className="mb-7">
        <h2 className="font-sans text-2xl font-semibold text-ink">
          {t('settings.contextWindow.title')}
        </h2>
        <p className="mt-2 font-sans text-sm text-ink-faint">
          {t('settings.contextWindow.description')}
        </p>
      </div>

      <form
        className="max-w-xl rounded-2xl border border-line bg-paper p-5"
        onSubmit={(event) => {
          event.preventDefault()
          if (parsed === null) return
          setContextWindowTokens(parsed)
          setSaved(true)
        }}
      >
        <div className="flex items-start gap-3">
          <CircleGauge size={21} className="mt-0.5 shrink-0 text-ink-soft" />
          <div className="min-w-0 flex-1">
            <label htmlFor="context-window-tokens" className="text-sm font-semibold text-ink">
              {t('settings.contextWindow.sizeLabel')}
            </label>
            <p className="mt-1 text-xs leading-5 text-ink-faint">
              {t('settings.contextWindow.sizeDescription')}
            </p>
            <div className="mt-4 flex items-center gap-2">
              <input
                id="context-window-tokens"
                type="number"
                min="1"
                step="1000"
                inputMode="numeric"
                value={draft}
                aria-invalid={parsed === null}
                className="h-10 min-w-0 flex-1 rounded-lg border border-line-strong bg-paper px-3 font-mono text-sm text-ink outline-none focus:ring-3 focus:ring-clay/20"
                onChange={(event) => {
                  setDraft(event.target.value)
                  setSaved(false)
                }}
              />
              <span className="shrink-0 text-sm text-ink-faint">Tokens</span>
              <button
                type="submit"
                disabled={parsed === null || parsed === contextWindowTokens}
                className="h-10 shrink-0 rounded-lg bg-clay px-4 text-sm font-medium text-white transition hover:bg-clay/90 disabled:cursor-not-allowed disabled:opacity-45"
              >
                {t('settings.contextWindow.save')}
              </button>
            </div>
            {parsed === null ? (
              <p className="mt-2 text-xs text-status-danger-ink" role="alert">
                {t('settings.contextWindow.invalid')}
              </p>
            ) : saved ? (
              <p className="mt-2 text-xs text-status-success-ink" role="status">
                {t('settings.contextWindow.saved')}
              </p>
            ) : null}
          </div>
        </div>
      </form>

      <p className="mt-4 max-w-xl text-xs leading-5 text-ink-faint">
        {t('settings.contextWindow.observationOnly')}
      </p>
    </div>
  )
}
