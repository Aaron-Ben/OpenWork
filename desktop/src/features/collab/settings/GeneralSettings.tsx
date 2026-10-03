import {
  Check,
  Languages,
  Monitor,
  Moon,
  Palette,
  Sun,
  type LucideIcon,
} from 'lucide-react'
import { type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

import { setLanguage, type SupportedLanguage } from '@/i18n'
import { useThemeStore, type Theme } from '@/app/themeStore'

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
