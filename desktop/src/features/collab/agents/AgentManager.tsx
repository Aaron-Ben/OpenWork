import { Plus } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabAgent, CollabAgentInput } from '@/bridge/collab'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { useCollabRuntimeStore } from '@/features/collab/runtimeStore'
import { AgentCard } from './AgentCard'
import { useAgentStore } from './agentStore'

const DEFAULT_MODEL = 'deepseek/deepseek-v4-flash'

function emptyAgent(): CollabAgentInput {
  return {
    displayName: '',
    role: null,
    persona: '',
    engineId: 'opencode',
    mainModelId: DEFAULT_MODEL,
    triageModelId: DEFAULT_MODEL,
  }
}

function editableAgent(agent: CollabAgent): CollabAgentInput {
  return {
    displayName: agent.displayName,
    role: agent.role,
    persona: agent.persona,
    engineId: agent.engineId,
    mainModelId: agent.mainModelId,
    triageModelId: agent.triageModelId,
  }
}

interface AgentFormState {
  agentId: string | null
  input: CollabAgentInput
}

export function AgentManager() {
  const { t } = useTranslation()
  const agents = useAgentStore((state) => state.agents)
  const create = useAgentStore((state) => state.create)
  const update = useAgentStore((state) => state.update)
  const error = useAgentStore((state) => state.error)
  const runtime = useCollabRuntimeStore((state) => state.status)
  const runtimeError = useCollabRuntimeStore((state) => state.error)
  const [form, setForm] = useState<AgentFormState | null>(null)

  async function submit(event: React.FormEvent) {
    event.preventDefault()
    if (!form) return
    if (form.agentId) await update(form.agentId, form.input)
    else await create(form.input)
    setForm(null)
  }

  function changeForm(patch: Partial<CollabAgentInput>) {
    setForm((current) => current ? {
      ...current,
      input: { ...current.input, ...patch },
    } : null)
  }

  return (
    <section className="min-w-0 flex-1 overflow-y-auto bg-paper">
      <header data-tauri-drag-region="deep" className="flex h-12 items-center justify-between border-b border-line px-6">
        <h1 className="font-serif text-lg font-semibold">{t('collab.agents.title')}</h1>
        <div className="flex items-center gap-3">
          <span className="text-xs text-ink-faint" title={runtime?.runtimeSessionId}>
            {runtime?.lastComputerHeartbeat ? t('collab.agents.runtimeReady') : t('collab.agents.runtimeStarting')}
          </span>
          <Button type="button" size="sm" onClick={() => setForm({ agentId: null, input: emptyAgent() })}>
            <Plus size={15} />{t('collab.agents.create')}
          </Button>
        </div>
      </header>
      <div className="mx-auto grid max-w-6xl gap-5 p-6">
        {error ? <p className="text-sm text-red-600">{error}</p> : null}
        {runtimeError ? <p className="text-sm text-red-600">{runtimeError}</p> : null}
        {agents.length > 0 ? (
          <div className="flex flex-wrap gap-2 text-xs text-ink-muted">
            <span className="rounded-full border border-line bg-paper-hover px-3 py-1.5">
              {t('collab.agents.activeCount', { count: agents.filter((agent) => agent.archivedAt === null).length })}
            </span>
            <span className="rounded-full border border-status-success-border bg-status-success-soft px-3 py-1.5 text-status-success-ink">
              {t('collab.agents.runningCount', { count: runtime?.runners.filter((runner) => runner.state === 'running').length ?? 0 })}
            </span>
          </div>
        ) : null}
        <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
          {agents.map((agent) => (
            <AgentCard
              key={agent.id}
              agent={agent}
              runtime={runtime}
              onEdit={() => setForm({ agentId: agent.id, input: editableAgent(agent) })}
            />
          ))}
        </div>
        {agents.length === 0 ? <p className="py-20 text-center text-sm text-ink-faint">{t('collab.agents.noAgents')}</p> : null}
      </div>
      {form ? (
        <div className="absolute inset-0 z-30 grid place-items-center bg-black/30 p-6" role="dialog" aria-modal="true">
          <form className="grid w-full max-w-xl gap-3 rounded-3xl bg-paper p-6 shadow-xl" onSubmit={submit}>
            <h2 className="font-serif text-xl font-semibold">{t(form.agentId ? 'collab.agents.edit' : 'collab.agents.create')}</h2>
            <Field label={t('collab.agents.displayName')}>
              <Input required value={form.input.displayName} onChange={(event) => changeForm({ displayName: event.target.value })} />
            </Field>
            <Field label={t('collab.agents.role')}>
              <Input value={form.input.role ?? ''} onChange={(event) => changeForm({ role: event.target.value || null })} />
            </Field>
            <Field label={t('collab.agents.mainModel')}>
              <Input required value={form.input.mainModelId} onChange={(event) => changeForm({ mainModelId: event.target.value })} />
            </Field>
            <Field label={t('collab.agents.triageModel')}>
              <Input required value={form.input.triageModelId} onChange={(event) => changeForm({ triageModelId: event.target.value })} />
            </Field>
            <Field label={t('collab.agents.persona')}>
              <Textarea required rows={5} value={form.input.persona} onChange={(event) => changeForm({ persona: event.target.value })} />
            </Field>
            <p className="text-xs text-ink-faint">Local Computer · OpenCode CLI · ID generated by Server</p>
            <div className="flex justify-end gap-2 pt-2">
              <Button type="button" variant="ghost" onClick={() => setForm(null)}>{t('common.cancel')}</Button>
              <Button type="submit" variant="accent" disabled={!form.input.displayName.trim() || !form.input.mainModelId.trim() || !form.input.triageModelId.trim()}>{t('collab.agents.save')}</Button>
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
