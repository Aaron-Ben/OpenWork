import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'

import { enUS } from './locales/en-US'
import { zhCN } from './locales/zh-CN'
import { zhTW } from './locales/zh-TW'

export const supportedLanguages = ['zh-CN', 'zh-TW', 'en-US'] as const
export type SupportedLanguage = (typeof supportedLanguages)[number]

const LANGUAGE_STORAGE_KEY = 'openwork-language'

function isSupportedLanguage(value: string | null): value is SupportedLanguage {
  return supportedLanguages.includes(value as SupportedLanguage)
}

function readStoredLanguage(): SupportedLanguage {
  if (typeof localStorage === 'undefined') return 'zh-CN'
  const stored = localStorage.getItem(LANGUAGE_STORAGE_KEY)
  return isSupportedLanguage(stored) ? stored : 'zh-CN'
}

const initialLanguage = readStoredLanguage()

void i18n.use(initReactI18next).init({
  resources: {
    'zh-CN': { translation: zhCN },
    'zh-TW': { translation: zhTW },
    'en-US': { translation: enUS },
  },
  lng: initialLanguage,
  fallbackLng: 'zh-CN',
  supportedLngs: [...supportedLanguages],
  load: 'currentOnly',
  interpolation: { escapeValue: false },
  react: { useSuspense: false },
  initAsync: false,
})

if (typeof document !== 'undefined') document.documentElement.lang = initialLanguage

export async function setLanguage(language: SupportedLanguage): Promise<void> {
  if (typeof localStorage !== 'undefined') localStorage.setItem(LANGUAGE_STORAGE_KEY, language)
  if (typeof document !== 'undefined') document.documentElement.lang = language
  await i18n.changeLanguage(language)
}

export default i18n
