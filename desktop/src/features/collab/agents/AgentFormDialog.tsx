import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabAgent, CollabAgentInput } from '@/bridge/collab'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Textarea } from '@/components/ui/textarea'

/** 新建 Agent 时主模型与判断模型的默认值（collaboration-desktop.md §8）。 */
const DEFAULT_MODEL = 'deepseek/deepseek-flash'

function inputFor(agent: CollabAgent | null): CollabAgentInput {
  if (!agent) {
    return { displayName: '', role: null, persona: '', engineId: 'opencode', mainModelId: DEFAULT_MODEL, triageModelId: DEFAULT_MODEL }
  }
  return {
    displayName: agent.displayName,
    role: agent.role,
    persona: agent.persona,
    engineId: agent.engineId,
    mainModelId: agent.mainModelId,
    triageModelId: agent.triageModelId,
  }
}

/**
 * 新建或编辑 Agent：显示名、role、persona、Engine（目前只有 OpenCode）、主模型与判断模型。
 * Persona 只是用户人格部分，协作契约由 Computer 追加（collaboration-desktop.md §8）。
 */
export function AgentFormDialog({ agent, error, onSubmit, onClose }: {
  agent: CollabAgent | null
  error: string | null
  onSubmit: (input: CollabAgentInput) => Promise<void>
  onClose: () => void
}) {
  const { t } = useTranslation()
  const [input, setInput] = useState(() => inputFor(agent))
  const change = (patch: Partial<CollabAgentInput>) => setInput((current) => ({ ...current, ...patch }))
  const ready = input.displayName.trim() && input.persona.trim() && input.mainModelId.trim() && input.triageModelId.trim()

  async function submit(event: React.FormEvent) {
    event.preventDefault()
    if (ready) await onSubmit(input)
  }

  return (
    <div className="absolute inset-0 z-30 grid place-items-center bg-black/30 p-6" role="dialog" aria-modal="true">
      <form className="grid w-full max-w-xl gap-3 rounded-3xl bg-paper p-6 shadow-xl" onSubmit={(event) => void submit(event)}>
        <h2 className="font-serif text-xl font-semibold">{t(agent ? 'collab.agents.edit' : 'collab.agents.create')}</h2>
        <Field label={t('collab.agents.displayName')}>
          <Input required value={input.displayName} onChange={(event) => change({ displayName: event.target.value })} />
        </Field>
        <Field label={t('collab.agents.role')}>
          <Input value={input.role ?? ''} onChange={(event) => change({ role: event.target.value || null })} />
        </Field>
        <Field label={t('collab.agents.engine.label')}>
          <Select value={input.engineId} onValueChange={() => change({ engineId: 'opencode' })}>
            <SelectTrigger className="h-9 w-full border border-line bg-paper px-3 text-sm"><SelectValue /></SelectTrigger>
            <SelectContent sideOffset={5}><SelectItem value="opencode">OpenCode</SelectItem></SelectContent>
          </Select>
        </Field>
        <div className="grid grid-cols-2 gap-3">
          <Field label={t('collab.agents.mainModel')}>
            <Input required placeholder="provider/model" value={input.mainModelId} onChange={(event) => change({ mainModelId: event.target.value })} />
          </Field>
          <Field label={t('collab.agents.triageModel')}>
            <Input required placeholder="provider/model" value={input.triageModelId} onChange={(event) => change({ triageModelId: event.target.value })} />
          </Field>
        </div>
        <Field label={t('collab.agents.persona')}>
          <Textarea required rows={6} value={input.persona} onChange={(event) => change({ persona: event.target.value })} />
        </Field>
        <p className="text-xs text-ink-faint">{t('collab.agents.personaHint')}</p>
        {error ? <p className="text-sm text-status-danger-ink">{error}</p> : null}
        <div className="flex justify-end gap-2 pt-2">
          <Button type="button" variant="ghost" onClick={onClose}>{t('common.cancel')}</Button>
          <Button type="submit" variant="accent" disabled={!ready}>{t('collab.agents.save')}</Button>
        </div>
      </form>
    </div>
  )
}

function Field({ label, children }: { label: string, children: React.ReactNode }) {
  return <label className="grid gap-1 text-xs font-medium text-ink-soft"><span>{label}</span>{children}</label>
}
