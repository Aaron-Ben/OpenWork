import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import type { CollabTriageSettings } from '@/bridge/collab'
import type { ProviderConfig } from '@/bridge/providerContracts'
import { selectTriageModelOptions, useTriageStore } from './triageStore'

export function TriageSettingsPanel() {
  const settings = useTriageStore((state) => state.settings)
  const providers = useTriageStore((state) => state.providers)
  const loading = useTriageStore((state) => state.loading)
  const saving = useTriageStore((state) => state.saving)
  const error = useTriageStore((state) => state.error)
  const fetch = useTriageStore((state) => state.fetch)
  const save = useTriageStore((state) => state.save)

  useEffect(() => {
    void fetch()
  }, [fetch])

  return (
    <TriageSettingsContent
      settings={settings}
      providers={providers}
      loading={loading}
      saving={saving}
      error={error}
      onSave={save}
    />
  )
}

export function TriageSettingsContent({ settings, providers, loading, saving, error, onSave }: {
  settings: CollabTriageSettings | null
  providers: readonly ProviderConfig[]
  loading: boolean
  saving: boolean
  error: string | null
  onSave: (providerId: string, modelId: string) => Promise<boolean>
}) {
  const { t } = useTranslation()
  const options = useMemo(() => selectTriageModelOptions(providers), [providers])
  const initialProviderId = options.some((option) => option.providerId === settings?.providerId)
    ? settings?.providerId ?? ''
    : options[0]?.providerId ?? ''
  const initialModelId = options.some((option) => (
    option.providerId === initialProviderId && option.modelId === settings?.modelId
  )) ? settings?.modelId ?? '' : options.find((option) => option.providerId === initialProviderId)?.modelId ?? ''
  const [providerId, setProviderId] = useState(initialProviderId)
  const [modelId, setModelId] = useState(initialModelId)

  const providerOptions = useMemo(() => (
    [...new Map(options.map((option) => [option.providerId, option.providerName])).entries()]
  ), [options])
  const modelOptions = options.filter((option) => option.providerId === providerId)
  const providerName = providerOptions.find(([id]) => id === providerId)?.[1]
  const modelName = modelOptions.find((option) => option.modelId === modelId)?.modelName

  useEffect(() => {
    const nextProviderId = options.some((option) => option.providerId === settings?.providerId)
      ? settings?.providerId ?? ''
      : options[0]?.providerId ?? ''
    const nextModelId = options.some((option) => (
      option.providerId === nextProviderId && option.modelId === settings?.modelId
    )) ? settings?.modelId ?? '' : options.find((option) => option.providerId === nextProviderId)?.modelId ?? ''
    setProviderId(nextProviderId)
    setModelId(nextModelId)
  }, [options, settings])

  async function submit(event: React.FormEvent) {
    event.preventDefault()
    if (!providerId || !modelId) return
    await onSave(providerId, modelId)
  }

  function selectProvider(nextProviderId: string) {
    setProviderId(nextProviderId)
    setModelId(options.find((option) => option.providerId === nextProviderId)?.modelId ?? '')
  }

  return (
    <section
      className="rounded-xl border border-line bg-paper px-4 py-3"
      data-triage-layout="compact"
      title={t('collab.triage.description')}
    >
      <div className="grid gap-3 lg:grid-cols-[minmax(12rem,0.7fr)_minmax(24rem,1.3fr)] lg:items-end">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <h2 className="text-sm font-semibold">{t('collab.triage.title')}</h2>
            <span
              className={settings
                ? 'rounded-full bg-green-50 px-2 py-0.5 text-[11px] font-medium text-green-800'
                : 'rounded-full bg-amber-50 px-2 py-0.5 text-[11px] font-medium text-amber-900'}
              data-triage-status
              data-triage-unconfigured={!settings ? 'true' : undefined}
            >
              {settings ? t('collab.triage.configured') : t('collab.triage.failOpen')}
            </span>
          </div>
          <p className="mt-1 truncate text-xs text-ink-faint">
            {settings
              ? t('collab.triage.current', { provider: settings.providerId, model: settings.modelId })
              : t('collab.triage.unconfigured')}
          </p>
        </div>
        <form className="grid gap-2 sm:grid-cols-[minmax(9rem,1fr)_minmax(9rem,1fr)_auto] sm:items-end" onSubmit={submit}>
          <label className="grid gap-1 text-xs font-medium text-ink-muted">
            <span>{t('collab.triage.provider')}</span>
            <Select
              disabled={loading || providerOptions.length === 0}
              value={providerId}
              onValueChange={selectProvider}
            >
              <SelectTrigger className="w-full rounded-lg border border-line bg-paper-hover font-normal" data-triage-provider>
                <SelectValue placeholder={t('collab.triage.provider')}>{providerName}</SelectValue>
              </SelectTrigger>
              <SelectContent>
                {providerOptions.map(([id, name]) => <SelectItem key={id} value={id}>{name}</SelectItem>)}
              </SelectContent>
            </Select>
          </label>
          <label className="grid gap-1 text-xs font-medium text-ink-muted">
            <span>{t('collab.triage.model')}</span>
            <Select
              disabled={loading || modelOptions.length === 0}
              value={modelId}
              onValueChange={setModelId}
            >
              <SelectTrigger className="w-full rounded-lg border border-line bg-paper-hover font-normal" data-triage-model>
                <SelectValue placeholder={t('collab.triage.model')}>{modelName}</SelectValue>
              </SelectTrigger>
              <SelectContent>
                {modelOptions.map((option) => <SelectItem key={option.modelId} value={option.modelId}>{option.modelName}</SelectItem>)}
              </SelectContent>
            </Select>
          </label>
          <Button type="submit" size="sm" variant="accent" disabled={loading || saving || !providerId || !modelId}>
            {saving ? t('collab.triage.saving') : t('collab.triage.save')}
          </Button>
        </form>
      </div>
      {!loading && options.length === 0 ? <p className="mt-2 text-xs text-ink-faint">{t('collab.triage.noModels')}</p> : null}
      {error ? (
        <details className="mt-2 rounded-lg bg-red-50 px-3 py-2 text-xs text-red-800" data-triage-error-details="true">
          <summary className="cursor-pointer font-medium">{t('collab.triage.errorSummary')}</summary>
          <pre className="mt-2 max-h-24 overflow-auto whitespace-pre-wrap break-all font-mono text-[11px] leading-relaxed">{error}</pre>
        </details>
      ) : null}
    </section>
  )
}
