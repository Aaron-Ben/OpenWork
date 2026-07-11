import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ChevronRight, Terminal } from 'lucide-react'

import { extractText, type ToolResultBlock } from '../../type/parts'

interface ToolResultViewProps {
  part: ToolResultBlock
}

/// 渲染一个 tool_result part(工具执行输出)。流式时与 tool_call 相邻出现在同一 assistant item;
/// 持久化后作为独立的 tool message(role:tool)渲染。
export function ToolResultView({ part }: ToolResultViewProps) {
  const { t } = useTranslation()
  const [expanded, setExpanded] = useState(false)
  const output = extractText(part.output)
  const isError = part.state === 'error'
  const hasOutput = output.length > 0

  return (
    <div className="overflow-hidden rounded-lg border border-line bg-paper-hover">
      <button
        type="button"
        onClick={() => hasOutput && setExpanded((value) => !value)}
        className="flex w-full items-center gap-2 px-3 py-2 text-left hover:bg-paper"
      >
        <Terminal size={14} className="shrink-0 text-ink-faint" />
        <span className="text-xs font-semibold text-ink-soft">{part.name}</span>
        <span className="text-[10px] text-ink-faint">{t('tool.result')}</span>
        <span className="flex-1" />
        <span className={`text-[10px] ${isError ? 'text-rose-500' : 'text-emerald-600'}`}>
          {isError ? t('tool.error') : t('tool.done')}
        </span>
        {hasOutput ? (
          <ChevronRight
            size={14}
            className={`text-ink-faint transition-transform ${expanded ? 'rotate-90' : ''}`}
          />
        ) : null}
      </button>
      {hasOutput && expanded ? (
        <div className="border-t border-line px-3 py-2">
          <pre
            className={`max-h-72 overflow-auto whitespace-pre-wrap break-words px-1 py-1 font-mono text-[11px] leading-relaxed ${
              isError ? 'text-rose-600' : 'text-ink-soft'
            }`}
          >
            {output}
          </pre>
        </div>
      ) : null}
    </div>
  )
}
