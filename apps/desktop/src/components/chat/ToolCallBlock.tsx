import { memo, useState } from 'react'
import {
  AlertCircle,
  CheckCircle2,
  ChevronRight,
  FileText,
  Folder,
  Loader2,
  Pencil,
  Terminal,
} from 'lucide-react'

export interface ToolCallResult {
  output: string
  isError: boolean
}

export interface ToolCallState {
  id: string
  toolName: string
  /** 累积的工具输入 JSON 片段(tool_call_delta 拼接而成)。 */
  partialInput: string
  result: ToolCallResult | null
}

interface ToolCallBlockProps {
  toolCall: ToolCallState
}

const TOOL_ICONS: Record<string, typeof Terminal> = {
  read: FileText,
  write: Pencil,
  list: Folder,
  bash: Terminal,
}

function parseInput(partial: string): Record<string, unknown> | null {
  if (!partial) return null
  try {
    const parsed = JSON.parse(partial)
    return parsed && typeof parsed === 'object'
      ? (parsed as Record<string, unknown>)
      : null
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
  const [expanded, setExpanded] = useState(false)
  const Icon = TOOL_ICONS[toolCall.toolName] ?? Terminal
  const parsed = parseInput(toolCall.partialInput)
  const summary = summarize(toolCall.toolName, parsed)
  const isPending = !toolCall.result
  const hasDetails = Boolean(parsed || toolCall.result?.output)

  return (
    <div className="overflow-hidden rounded-lg border border-line bg-paper-hover">
      <button
        type="button"
        onClick={() => hasDetails && setExpanded((value) => !value)}
        className="flex w-full items-center gap-2 px-3 py-2 text-left hover:bg-paper"
      >
        <Icon size={14} className="shrink-0 text-ink-faint" />
        <span className="text-xs font-semibold text-ink-soft">{toolCall.toolName}</span>
        {summary ? (
          <span className="min-w-0 flex-1 truncate font-mono text-xs text-ink-faint">{summary}</span>
        ) : (
          <span className="flex-1" />
        )}
        {isPending ? (
          <span className="inline-flex shrink-0 items-center gap-1 text-[10px] text-ink-faint">
            <Loader2 size={11} className="animate-spin" />
            running
          </span>
        ) : toolCall.result?.isError ? (
          <span className="inline-flex shrink-0 items-center gap-1 text-[10px] text-rose-500">
            <AlertCircle size={11} />
            error
          </span>
        ) : (
          <span className="inline-flex shrink-0 items-center gap-1 text-[10px] text-emerald-600">
            <CheckCircle2 size={11} />
            done
          </span>
        )}
        {hasDetails ? (
          <ChevronRight
            size={14}
            className={`text-ink-faint transition-transform ${expanded ? 'rotate-90' : ''}`}
          />
        ) : null}
      </button>
      {hasDetails && expanded ? (
        <div className="space-y-2 border-t border-line px-3 py-2">
          {parsed ? <InputView toolName={toolCall.toolName} input={parsed} /> : null}
          {toolCall.result?.output ? (
            <OutputView output={toolCall.result.output} isError={toolCall.result.isError} />
          ) : null}
        </div>
      ) : null}
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
  if (toolName === 'bash' && typeof input.command === 'string') {
    return (
      <div className="overflow-hidden rounded-md border border-line bg-ink px-3 py-2 font-mono text-[11px] leading-relaxed text-paper">
        <span className="text-emerald-400">$</span> {input.command}
      </div>
    )
  }
  const value =
    toolName === 'write' && typeof input.content === 'string'
      ? input.content
      : JSON.stringify(input, null, 2)
  const label = toolName === 'write' ? 'content' : 'input'
  return (
    <div className="overflow-hidden rounded-md border border-line bg-paper">
      <div className="border-b border-line px-3 py-1.5 text-[10px] uppercase tracking-wider text-ink-faint">
        {label}
      </div>
      <pre className="max-h-72 overflow-auto whitespace-pre-wrap break-words px-3 py-2 font-mono text-[11px] leading-relaxed text-ink-soft">
        {value}
      </pre>
    </div>
  )
}

function OutputView({ output, isError }: { output: string; isError: boolean }) {
  return (
    <div
      className={`overflow-hidden rounded-md border ${
        isError ? 'border-rose-200 bg-rose-50' : 'border-line bg-paper'
      }`}
    >
      <div
        className={`border-b px-3 py-1.5 text-[10px] uppercase tracking-wider ${
          isError ? 'border-rose-200 text-rose-500' : 'border-line text-ink-faint'
        }`}
      >
        {isError ? 'error output' : 'output'}
      </div>
      <pre
        className={`max-h-72 overflow-auto whitespace-pre-wrap break-words px-3 py-2 font-mono text-[11px] leading-relaxed ${
          isError ? 'text-rose-600' : 'text-ink-soft'
        }`}
      >
        {output}
      </pre>
    </div>
  )
}
