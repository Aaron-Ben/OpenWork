import { Bot, Plus } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabAgentInput } from '@/bridge/collab'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { deriveAgentSlug } from './agentId'
import { useAgentStore } from './agentStore'

const DEFAULT_MODEL = 'opencode/mimo-v2.5-free'

export function AgentManager() {
  const { t } = useTranslation()
  const agents = useAgentStore((state) => state.agents)
  const create = useAgentStore((state) => state.create)
  const error = useAgentStore((state) => state.error)
  const setProactivity = useAgentStore((state) => state.setProactivity)
  const [form, setForm] = useState<CollabAgentInput | null>(null)

  async function submit(event: React.FormEvent) {
    event.preventDefault()
    if (!form) return
    const id = deriveAgentSlug(form.displayName) ?? form.id
    await create({ ...form, id })
    setForm(null)
  }

  const derivedId = form ? deriveAgentSlug(form.displayName) : null
  const effectiveId = derivedId ?? form?.id ?? ''

  return (
    <section className="min-w-0 flex-1 overflow-y-auto bg-paper">
      <header data-tauri-drag-region="deep" className="flex h-12 items-center justify-between border-b border-line px-6">
        <h1 className="font-serif text-lg font-semibold">{t('collab.agents.title')}</h1>
        <Button type="button" size="sm" onClick={() => setForm({ id: '', displayName: '', systemPrompt: '', model: DEFAULT_MODEL })}>
          <Plus size={15} />{t('collab.agents.create')}
        </Button>
      </header>
      <div className="mx-auto grid max-w-4xl gap-3 p-6">
        {error ? <p className="text-sm text-red-600">{error}</p> : null}
        {agents.map((agent) => (
          <article key={agent.id} className="flex items-center gap-4 rounded-2xl border border-line bg-paper-hover p-4">
            <span className="grid size-10 place-items-center rounded-full bg-clay/10 text-clay"><Bot size={19} /></span>
            <div className="min-w-0 flex-1">
              <strong className="block truncate">{agent.displayName}</strong>
              <span className="text-sm text-ink-faint">@{agent.id} · OpenCode · {agent.model} · {agent.enabled ? t('collab.agents.enabled') : t('collab.agents.disabled')}</span>
            </div>
            <Button type="button" size="sm" variant={agent.scannerEnabled ? 'accent' : 'outline'} onClick={() => void setProactivity(agent.id, !agent.scannerEnabled)}>
              {agent.scannerEnabled ? t('collab.agents.proactiveOn') : t('collab.agents.proactiveOff')}
            </Button>
          </article>
        ))}
        {agents.length === 0 ? <p className="py-20 text-center text-sm text-ink-faint">{t('collab.agents.noAgents')}</p> : null}
      </div>
      {form ? (
        <div className="absolute inset-0 z-30 grid place-items-center bg-black/30 p-6" role="dialog" aria-modal="true">
          <form className="grid w-full max-w-xl gap-3 rounded-3xl bg-paper p-6 shadow-xl" onSubmit={submit}>
            <h2 className="font-serif text-xl font-semibold">{t('collab.agents.create')}</h2>
            <Field label={t('collab.agents.displayName')}>
              <Input required value={form.displayName} onChange={(event) => setForm({ ...form, displayName: event.target.value })} />
            </Field>
            <Field label={t('collab.agents.id')}>
              <Input required pattern="[a-z][a-z0-9_]{0,47}" value={effectiveId} onChange={(event) => setForm({ ...form, id: event.target.value })} readOnly={derivedId !== null} />
            </Field>
            <Field label={t('collab.agents.model')}>
              <Input required value={form.model} onChange={(event) => setForm({ ...form, model: event.target.value })} />
            </Field>
            <Field label={t('collab.agents.prompt')}>
              <Textarea required rows={5} value={form.systemPrompt} onChange={(event) => setForm({ ...form, systemPrompt: event.target.value })} />
            </Field>
            <p className="text-xs text-ink-faint">Local Computer · OpenCode CLI</p>
            <div className="flex justify-end gap-2 pt-2">
              <Button type="button" variant="ghost" onClick={() => setForm(null)}>{t('common.cancel')}</Button>
              <Button type="submit" variant="accent" disabled={!effectiveId || !form.model.trim()}>{t('collab.agents.save')}</Button>
            </div>
          </form>
        </div>
      ) : null}
    </section>
  )
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return <label className="grid gap-1 text-xs font-medium text-ink-muted"><span>{label}</span>{children}</label>
}
