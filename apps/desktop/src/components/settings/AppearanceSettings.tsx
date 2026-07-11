import { Check, Languages, Monitor, Moon, Sun } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { useThemeStore, type Theme } from '../../stores/themeStore'
import { setLanguage, type SupportedLanguage } from '../../i18n'

export function AppearanceSettings() {
  const { t, i18n } = useTranslation()
  const theme = useThemeStore((state) => state.theme)
  const setTheme = useThemeStore((state) => state.setTheme)
  const choices: Array<{ value: Theme; label: string; description: string; icon: typeof Sun }> = [
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
    <div className="mx-auto w-full max-w-4xl p-8 max-[640px]:p-5">
      <div className="mb-7">
        <h2 className="font-sans text-2xl font-semibold text-ink">{t('settings.appearance.title')}</h2>
        <p className="mt-2 font-sans text-sm text-ink-faint">{t('settings.appearance.description')}</p>
      </div>
      <div role="radiogroup" aria-label={t('settings.appearance.themeLabel')} className="grid gap-3 sm:grid-cols-3">
        {choices.map((choice) => {
          const Icon = choice.icon
          const selected = theme === choice.value
          return (
            <button
              key={choice.value}
              type="button"
              role="radio"
              aria-checked={selected}
              className={`relative rounded-2xl border p-5 text-left transition ${
                selected ? 'border-clay bg-clay-soft' : 'border-line bg-paper hover:bg-paper-hover'
              }`}
              onClick={() => setTheme(choice.value)}
            >
              <Icon size={22} className={selected ? 'text-clay' : 'text-ink-soft'} />
              <div className="mt-6 font-sans text-sm font-semibold text-ink">{choice.label}</div>
              <div className="mt-1 font-sans text-xs leading-5 text-ink-faint">{choice.description}</div>
              {selected ? <Check size={17} className="absolute right-4 top-4 text-clay" /> : null}
            </button>
          )
        })}
      </div>
      <div className="mb-5 mt-10">
        <div className="flex items-center gap-2">
          <Languages size={20} className="text-ink-soft" />
          <h3 className="font-sans text-lg font-semibold text-ink">{t('settings.appearance.language')}</h3>
        </div>
        <p className="mt-2 font-sans text-sm text-ink-faint">{t('settings.appearance.languageDescription')}</p>
      </div>
      <div role="radiogroup" aria-label={t('settings.appearance.language')} className="grid gap-3 sm:grid-cols-3">
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
    </div>
  )
}
