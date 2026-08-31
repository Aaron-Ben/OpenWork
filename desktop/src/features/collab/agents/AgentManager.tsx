import { Bot, Pencil, Plus } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabAgent, CollabAgentInput, CollabRuntimeStatus } from '@/bridge/collab'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { useCollabRuntimeStore } from '@/features/collab/runtimeStore'
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
  const setAgenda = useAgentStore((state) => state.setAgenda)
  const setArchived = useAgentStore((state) => state.setArchived)
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
      <div className="mx-auto grid max-w-4xl gap-3 p-6">
        {error ? <p className="text-sm text-red-600">{error}</p> : null}
        {runtimeError ? <p className="text-sm text-red-600">{runtimeError}</p> : null}
        {agents.map((agent) => {
          const archived = agent.archivedAt !== null
          const actual = actualState(runtime, agent)
          const actualLabel = archived
            ? null
            : actual.kind === 'running'
              ? t('collab.agents.running')
              : actual.kind === 'error'
                ? `${t('collab.agents.runnerError')}: ${actual.detail ?? t('collab.rooms.unknownFailure')}`
                : t(`collab.agents.${actual.kind}`)
          return (
            <article key={agent.id} className="flex items-center gap-4 rounded-2xl border border-line bg-paper-hover p-4">
              <span className="grid size-10 place-items-center rounded-full bg-clay/10 text-clay"><Bot size={19} /></span>
              <div className="min-w-0 flex-1">
                <strong className="block truncate">{agent.displayName}</strong>
                <span className="text-sm text-ink-faint">
                  @{agent.id} · OpenCode · {agent.mainModelId} · {archived ? t('collab.agents.archived') : t('collab.agents.active')}
                </span>
                {actualLabel ? <span className="block truncate text-xs text-ink-faint" title={actualLabel}>{actualLabel}</span> : null}
              </div>
              <Button
                type="button"
                size="sm"
                variant="ghost"
                aria-label={t('collab.agents.edit')}
                onClick={() => setForm({ agentId: agent.id, input: editableAgent(agent) })}
              >
                <Pencil size={15} />
              </Button>
              <Button
                type="button"
                size="sm"
                disabled={archived}
                variant={agent.agendaEnabled ? 'accent' : 'outline'}
                onClick={() => void setAgenda(agent.id, !agent.agendaEnabled)}
              >
                {agent.agendaEnabled ? t('collab.agents.proactiveOn') : t('collab.agents.proactiveOff')}
              </Button>
              <Button type="button" size="sm" variant="outline" onClick={() => void setArchived(agent.id, !archived)}>
                {archived ? t('collab.agents.restore') : t('collab.agents.archive')}
              </Button>
            </article>
          )
        })}
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

type ActualState =
  | { kind: 'running' }
  | { kind: 'starting' | 'restarting' | 'engineMissing' | 'engineError' }
  | { kind: 'error'; detail: string | null }

function actualState(runtime: CollabRuntimeStatus | null, agent: CollabAgent): ActualState {
  const runner = runtime?.runners.find((candidate) => candidate.agentId === agent.id)
  if (runner?.state === 'error') {
    return { kind: 'error', detail: runner.lastError }
  }
  if (runner?.state === 'running') {
    return runner.configRevision === agent.configRevision
      ? { kind: 'running' }
      : { kind: 'restarting' }
  }
  const readiness = runtime?.engineReadiness.find((engine) => engine.engineId === agent.engineId)
  if (readiness?.status === 'missing') return { kind: 'engineMissing' }
  if (readiness?.status === 'error') return { kind: 'engineError' }
  return { kind: 'starting' }
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return <label className="grid gap-1 text-xs font-medium text-ink-muted"><span>{label}</span>{children}</label>
}
