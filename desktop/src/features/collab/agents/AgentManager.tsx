import { Bot, Pencil, Plus, Power } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabAgent, CollabAgentInput } from '@/bridge/collab'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { useAgentStore } from './agentStore'

const emptyAgent: CollabAgentInput = {
  id: '',
  displayName: '',
  role: null,
  bio: null,
  systemPrompt: '',
  providerId: '',
  modelId: '',
  enabled: true,
}

function editable(agent: CollabAgent): CollabAgentInput {
  const { opencodeSessionId: _, activity: _activity, ...input } = agent
  return input
}

export function AgentManager() {
  const { t } = useTranslation()
  const agents = useAgentStore((state) => state.agents)
  const create = useAgentStore((state) => state.create)
  const update = useAgentStore((state) => state.update)
  const error = useAgentStore((state) => state.error)
  const [form, setForm] = useState<CollabAgentInput | null>(null)
  const [editing, setEditing] = useState(false)

  async function submit(event: React.FormEvent) {
    event.preventDefault()
    if (!form) return
    if (editing) await update(form)
    else await create(form)
    setForm(null)
  }

  function patch(values: Partial<CollabAgentInput>) {
    setForm((current) => current ? { ...current, ...values } : current)
  }

  return (
    <section className="min-w-0 flex-1 overflow-y-auto bg-paper">
      <header data-tauri-drag-region="deep" className="flex h-12 items-center justify-between border-b border-line px-6">
        <h1 className="font-serif text-lg font-semibold">{t('collab.agents.title')}</h1>
        <Button type="button" size="sm" onClick={() => { setEditing(false); setForm(emptyAgent) }}><Plus size={15} />{t('collab.agents.create')}</Button>
      </header>
      <div className="mx-auto grid max-w-4xl gap-3 p-6">
        {error ? <p className="text-sm text-red-600">{error}</p> : null}
        {agents.map((agent) => (
          <article key={agent.id} className="flex items-center gap-4 rounded-2xl border border-line bg-paper-hover p-4">
            <span className="grid size-10 place-items-center rounded-full bg-clay/10 text-clay"><Bot size={19} /></span>
            <div className="min-w-0 flex-1">
              <strong className="block truncate">{agent.displayName}</strong>
              <span className="text-sm text-ink-faint">{agent.role || agent.id} · {agent.enabled ? t('collab.agents.enabled') : t('collab.agents.disabled')}</span>
            </div>
            <Button type="button" variant="ghost" size="sm" onClick={() => { setEditing(true); setForm(editable(agent)) }}><Pencil size={14} />{t('collab.agents.edit')}</Button>
            <Button type="button" variant="ghost" size="sm" onClick={() => void update({ ...editable(agent), enabled: !agent.enabled })}><Power size={14} />{agent.enabled ? t('collab.agents.disable') : t('collab.agents.enable')}</Button>
          </article>
        ))}
        {agents.length === 0 ? <p className="py-20 text-center text-sm text-ink-faint">{t('collab.agents.noAgents')}</p> : null}
      </div>
      {form ? (
        <div className="absolute inset-0 z-30 grid place-items-center bg-black/30 p-6" role="dialog" aria-modal="true">
          <form className="grid max-h-full w-full max-w-xl gap-3 overflow-y-auto rounded-3xl bg-paper p-6 shadow-xl" onSubmit={submit}>
            <h2 className="font-serif text-xl font-semibold">{editing ? t('collab.agents.edit') : t('collab.agents.create')}</h2>
            <Field label={t('collab.agents.id')}><Input required disabled={editing} pattern="[a-z][a-z0-9_]{0,47}" value={form.id} onChange={(event) => patch({ id: event.target.value })} /></Field>
            <Field label={t('collab.agents.displayName')}><Input required value={form.displayName} onChange={(event) => patch({ displayName: event.target.value })} /></Field>
            <div className="grid grid-cols-2 gap-3">
              <Field label={t('collab.agents.role')}><Input value={form.role ?? ''} onChange={(event) => patch({ role: event.target.value || null })} /></Field>
              <Field label={t('collab.agents.bio')}><Input value={form.bio ?? ''} onChange={(event) => patch({ bio: event.target.value || null })} /></Field>
            </div>
            <Field label={t('collab.agents.prompt')}><Textarea required rows={5} value={form.systemPrompt} onChange={(event) => patch({ systemPrompt: event.target.value })} /></Field>
            <div className="grid grid-cols-2 gap-3">
              <Field label={t('collab.agents.provider')}><Input required value={form.providerId} onChange={(event) => patch({ providerId: event.target.value })} /></Field>
              <Field label={t('collab.agents.model')}><Input required value={form.modelId} onChange={(event) => patch({ modelId: event.target.value })} /></Field>
            </div>
            <div className="flex justify-end gap-2 pt-2">
              <Button type="button" variant="ghost" onClick={() => setForm(null)}>{t('common.cancel')}</Button>
              <Button type="submit" variant="accent">{t('collab.agents.save')}</Button>
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
