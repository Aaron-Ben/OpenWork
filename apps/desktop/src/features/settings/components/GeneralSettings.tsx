import {
  Check,
  CircleGauge,
  FileWarning,
  Languages,
  Monitor,
  Moon,
  Palette,
  Sun,
  type LucideIcon,
} from 'lucide-react'
import { useEffect, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

import type { RuntimeTraceContentPolicy } from '@/bridge/compat'
import { setLanguage, type SupportedLanguage } from '@/i18n'
import {
  parseContextWindowTokens,
  useContextWindowStore,
} from '@/features/settings/contextWindowStore'
import { useThemeStore, type Theme } from '@/app/themeStore'
import { useTraceContentStore } from '@/features/settings/traceContentStore'

const TRACE_POLICIES: RuntimeTraceContentPolicy[] = ['full', 'compaction_only', 'off']

export function GeneralSettings() {
  const { t } = useTranslation()

  return (
    <div className="mx-auto w-full max-w-4xl p-8 max-[640px]:p-5">
      <div className="mb-7">
        <h2 className="font-sans text-2xl font-semibold text-ink">{t('settings.general.title')}</h2>
        <p className="mt-2 font-sans text-sm text-ink-faint">{t('settings.general.description')}</p>
      </div>
      <div className="grid gap-5 pb-8">
        <AppearanceSection />
        <ContextWindowSection />
        <TraceContentSection />
      </div>
    </div>
  )
}

function SettingsSection({ icon: Icon, title, description, children }: {
  icon: LucideIcon
  title: string
  description: string
  children: ReactNode
}) {
  return (
    <section className="overflow-hidden rounded-2xl border border-line bg-paper">
      <header className="flex items-start gap-3 border-b border-line px-5 py-4">
        <div className="grid size-9 shrink-0 place-items-center rounded-xl bg-clay-soft text-clay">
          <Icon size={18} />
        </div>
        <div className="min-w-0">
          <h3 className="font-sans text-sm font-semibold text-ink">{title}</h3>
          <p className="mt-1 font-sans text-xs leading-5 text-ink-faint">{description}</p>
        </div>
      </header>
      <div className="px-5 py-5">{children}</div>
    </section>
  )
}

function AppearanceSection() {
  const { t, i18n } = useTranslation()
  const theme = useThemeStore((state) => state.theme)
  const setTheme = useThemeStore((state) => state.setTheme)
  const choices: Array<{ value: Theme; label: string; description: string; icon: LucideIcon }> = [
    { value: 'light', label: t('settings.appearance.light'), description: t('settings.appearance.lightDescription'), icon: Sun },
    { value: 'dark', label: t('settings.appearance.dark'), description: t('settings.appearance.darkDescription'), icon: Moon },
    { value: 'system', label: t('settings.appearance.system'), description: t('settings.appearance.systemDescription'), icon: Monitor },
  ]
  const language = (i18n.resolvedLanguage ?? 'zh-CN') as SupportedLanguage
  const languages: Array<{ value: SupportedLanguage; label: string }> = [
    { value: 'zh-CN', label: t('settings.appearance.simplifiedChinese') },
    { value: 'zh-TW', label: t('settings.appearance.traditionalChinese') },
    { value: 'en-US', label: t('settings.appearance.english') },
  ]

  return (
    <SettingsSection
      icon={Palette}
      title={t('settings.appearance.title')}
      description={t('settings.appearance.description')}
    >
      <div className="font-sans text-xs font-medium text-ink-faint">{t('settings.appearance.themeLabel')}</div>
      <div role="radiogroup" aria-label={t('settings.appearance.themeLabel')} className="mt-2 grid gap-3 sm:grid-cols-3">
        {choices.map((choice) => {
          const Icon = choice.icon
          const selected = theme === choice.value
          return (
            <button
              key={choice.value}
              type="button"
              role="radio"
              aria-checked={selected}
              className={`relative rounded-2xl border p-4 text-left transition ${
                selected ? 'border-clay bg-clay-soft' : 'border-line bg-paper hover:bg-paper-hover'
              }`}
              onClick={() => setTheme(choice.value)}
            >
              <Icon size={20} className={selected ? 'text-clay' : 'text-ink-soft'} />
              <div className="mt-5 font-sans text-sm font-semibold text-ink">{choice.label}</div>
              <div className="mt-1 font-sans text-xs leading-5 text-ink-faint">{choice.description}</div>
              {selected ? <Check size={16} className="absolute right-3.5 top-3.5 text-clay" /> : null}
            </button>
          )
        })}
      </div>

      <div className="mt-6 flex items-center gap-2">
        <Languages size={15} className="text-ink-faint" />
        <div className="font-sans text-xs font-medium text-ink-faint">{t('settings.appearance.language')}</div>
      </div>
      <div role="radiogroup" aria-label={t('settings.appearance.language')} className="mt-2 grid gap-3 sm:grid-cols-3">
        {languages.map((option) => {
          const selected = language === option.value
          return (
            <button
              key={option.value}
              type="button"
              role="radio"
              aria-checked={selected}
              className={`relative rounded-xl border px-4 py-3 text-left font-sans text-sm font-medium transition ${
                selected ? 'border-clay bg-clay-soft text-ink' : 'border-line bg-paper text-ink-soft hover:bg-paper-hover'
              }`}
              onClick={() => void setLanguage(option.value)}
            >
              {option.label}
              {selected ? <Check size={16} className="absolute right-3 top-1/2 -translate-y-1/2 text-clay" /> : null}
            </button>
          )
        })}
      </div>
    </SettingsSection>
  )
}

function ContextWindowSection() {
  const { t } = useTranslation()
  const contextWindowTokens = useContextWindowStore((state) => state.contextWindowTokens)
  const setContextWindowTokens = useContextWindowStore((state) => state.setContextWindowTokens)
  const [draft, setDraft] = useState(String(contextWindowTokens))
  const [saved, setSaved] = useState(false)
  const parsed = parseContextWindowTokens(draft)

  useEffect(() => setDraft(String(contextWindowTokens)), [contextWindowTokens])

  return (
    <SettingsSection
      icon={CircleGauge}
      title={t('settings.contextWindow.title')}
      description={t('settings.contextWindow.description')}
    >
      <form
        onSubmit={(event) => {
          event.preventDefault()
          if (parsed === null) return
          setContextWindowTokens(parsed)
          setSaved(true)
        }}
      >
        <label htmlFor="context-window-tokens" className="font-sans text-sm font-semibold text-ink">
          {t('settings.contextWindow.sizeLabel')}
        </label>
        <p className="mt-1 font-sans text-xs leading-5 text-ink-faint">
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
          <span className="shrink-0 font-sans text-sm text-ink-faint">Tokens</span>
          <button
            type="submit"
            disabled={parsed === null || parsed === contextWindowTokens}
            className="h-10 shrink-0 rounded-lg bg-clay px-4 font-sans text-sm font-medium text-white transition hover:bg-clay/90 disabled:cursor-not-allowed disabled:opacity-45"
          >
            {t('settings.contextWindow.save')}
          </button>
        </div>
        {parsed === null ? (
          <p className="mt-2 font-sans text-xs text-status-danger-ink" role="alert">
            {t('settings.contextWindow.invalid')}
          </p>
        ) : saved ? (
          <p className="mt-2 font-sans text-xs text-status-success-ink" role="status">
            {t('settings.contextWindow.saved')}
          </p>
        ) : null}
      </form>
      <p className="mt-4 rounded-xl bg-paper-hover px-4 py-3 font-sans text-xs leading-5 text-ink-faint">
        {t('settings.contextWindow.observationOnly')}
      </p>
    </SettingsSection>
  )
}

function TraceContentSection() {
  const { t } = useTranslation()
  const policy = useTraceContentStore((state) => state.policy)
  const updating = useTraceContentStore((state) => state.updating)
  const error = useTraceContentStore((state) => state.error)
  const updatePolicy = useTraceContentStore((state) => state.updatePolicy)
  const [draft, setDraft] = useState<RuntimeTraceContentPolicy>(policy)
  const [saved, setSaved] = useState(false)

  useEffect(() => setDraft(policy), [policy])

  return (
    <SettingsSection
      icon={FileWarning}
      title={t('settings.traceContent.title')}
      description={t('settings.traceContent.description')}
    >
      <form
        onSubmit={(event) => {
          event.preventDefault()
          setSaved(false)
          void updatePolicy(draft).then(setSaved)
        }}
      >
        <div className="font-sans text-sm font-semibold text-ink">
          {t('settings.traceContent.policyLabel')}
        </div>
        <p className="mt-1 font-sans text-xs leading-5 text-ink-faint">
          {t('settings.traceContent.sourceWarning')}
        </p>
        <div role="radiogroup" aria-label={t('settings.traceContent.policyLabel')} className="mt-4 grid gap-2">
          {TRACE_POLICIES.map((value) => {
            const selected = draft === value
            return (
              <button
                key={value}
                type="button"
                role="radio"
                aria-checked={selected}
                disabled={updating}
                className={`flex items-start gap-3 rounded-xl border px-4 py-3 text-left transition disabled:cursor-not-allowed disabled:opacity-60 ${
                  selected ? 'border-clay bg-clay-soft' : 'border-line bg-paper hover:bg-paper-hover'
                }`}
                onClick={() => {
                  setDraft(value)
                  setSaved(false)
                }}
              >
                <span className="min-w-0 flex-1">
                  <span className="block font-sans text-sm font-medium text-ink">
                    {t(`settings.traceContent.policies.${value}.label`)}
                  </span>
                  <span className="mt-0.5 block font-sans text-xs leading-5 text-ink-faint">
                    {t(`settings.traceContent.policies.${value}.description`)}
                  </span>
                </span>
                {selected ? <Check size={16} className="mt-0.5 shrink-0 text-clay" /> : null}
              </button>
            )
          })}
        </div>
        <button
          type="submit"
          disabled={updating || draft === policy}
          className="mt-4 h-10 rounded-lg bg-clay px-4 font-sans text-sm font-medium text-white transition hover:bg-clay/90 disabled:cursor-not-allowed disabled:opacity-45"
        >
          {updating ? t('settings.traceContent.saving') : t('settings.traceContent.save')}
        </button>
        {error ? (
          <p className="mt-2 font-sans text-xs text-status-danger-ink" role="alert">{error}</p>
        ) : saved ? (
          <p className="mt-2 font-sans text-xs text-status-success-ink" role="status">
            {t('settings.traceContent.saved')}
          </p>
        ) : null}
      </form>
      <p className="mt-4 rounded-xl bg-paper-hover px-4 py-3 font-sans text-xs leading-5 text-ink-faint">
        {t('settings.traceContent.runtimeNote')}
      </p>
    </SettingsSection>
  )
}
