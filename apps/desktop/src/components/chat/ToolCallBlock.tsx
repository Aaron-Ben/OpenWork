import { memo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  CheckCircle2,
  ChevronRight,
  FileText,
  Folder,
  Loader2,
  Pencil,
  Terminal,
} from 'lucide-react'

import type { ToolCallBlock as ToolCallPart } from '../../type/parts'

interface ToolCallBlockProps {
  toolCall: ToolCallPart
}

const TOOL_ICONS: Record<string, typeof Terminal> = {
  read: FileText,
  write: Pencil,
  list: Folder,
  bash: Terminal,
}

function parseInput(input: string): Record<string, unknown> | null {
  if (!input) return null
  try {
    const parsed = JSON.parse(input)
    return parsed && typeof parsed === 'object' ? (parsed as Record<string, unknown>) : null
  } catch {
    return null
  }
}

function summarize(toolName: string, input: Record<string, unknown> | null): string {
  if (!input) return ''
  if (toolName === 'bash' && typeof input.command === 'string') return input.command
  if (typeof input.path === 'string') return input.path
  return ''
}

export const ToolCallBlock = memo(function ToolCallBlock({ toolCall }: ToolCallBlockProps) {
  const { t } = useTranslation()
  const [expanded, setExpanded] = useState(false)
  const Icon = TOOL_ICONS[toolCall.name] ?? Terminal
  const parsed = parseInput(toolCall.input)
  const summary = summarize(toolCall.name, parsed)
  const isPending = toolCall.state === 'pending' || toolCall.state === 'submitted'
  const hasDetails = Boolean(parsed)

  return (
    <div className="overflow-hidden rounded-lg border border-line bg-paper-hover">
      <button
        type="button"
        onClick={() => hasDetails && setExpanded((value) => !value)}
        className="flex w-full items-center gap-2 px-3 py-2 text-left hover:bg-paper"
      >
        <Icon size={14} className="shrink-0 text-ink-faint" />
        <span className="text-xs font-semibold text-ink-soft">{toolCall.name}</span>
        {summary ? (
          <span className="min-w-0 flex-1 truncate font-mono text-xs text-ink-faint">{summary}</span>
        ) : (
          <span className="flex-1" />
        )}
        {isPending ? (
          <span className="inline-flex shrink-0 items-center gap-1 text-[10px] text-ink-faint">
            <Loader2 size={11} className="animate-spin" />
            {t('tool.running')}
          </span>
        ) : (
          <span className="inline-flex shrink-0 items-center gap-1 text-[10px] text-emerald-600">
            <CheckCircle2 size={11} />
            {t('tool.done')}
          </span>
        )}
        {hasDetails ? (
          <ChevronRight
            size={14}
            className={`text-ink-faint transition-transform ${expanded ? 'rotate-90' : ''}`}
          />
        ) : null}
      </button>
      {parsed && expanded ? <InputView toolName={toolCall.name} input={parsed} /> : null}
    </div>
  )
})

function InputView({
  toolName,
  input,
}: {
  toolName: string
  input: Record<string, unknown>
}) {
  const { t } = useTranslation()
  if (toolName === 'bash' && typeof input.command === 'string') {
    return (
      <div className="border-t border-line px-3 py-2">
        <div className="overflow-hidden rounded-md border border-line bg-ink px-3 py-2 font-mono text-[11px] leading-relaxed text-paper">
          <span className="text-emerald-400">$</span> {input.command}
        </div>
      </div>
    )
  }
  const value =
    toolName === 'write' && typeof input.content === 'string'
      ? input.content
      : JSON.stringify(input, null, 2)
  const label = toolName === 'write' ? t('tool.content') : t('tool.input')
  return (
    <div className="border-t border-line px-3 py-2">
      <div className="overflow-hidden rounded-md border border-line bg-paper">
        <div className="border-b border-line px-3 py-1.5 text-[10px] uppercase tracking-wider text-ink-faint">
          {label}
        </div>
        <pre className="max-h-72 overflow-auto whitespace-pre-wrap break-words px-3 py-2 font-mono text-[11px] leading-relaxed text-ink-soft">
          {value}
        </pre>
      </div>
    </div>
  )
}
