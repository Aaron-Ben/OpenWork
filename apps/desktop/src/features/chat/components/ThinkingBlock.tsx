import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { MarkdownRenderer } from '../../../components/markdown/MarkdownRenderer'

interface ThinkingBlockProps {
  content: string
  isActive?: boolean
}

export function ThinkingBlock({ content, isActive = false }: ThinkingBlockProps) {
  const { t } = useTranslation()
  const [expanded, setExpanded] = useState(false)
  const contentRef = useRef<HTMLDivElement>(null)
  const displayContent = useMemo(() => content.replace(/\r\n?/g, '\n').trimEnd(), [content])
  const hasContent = displayContent.trim().length > 0

  useEffect(() => {
    if (expanded && isActive && contentRef.current) {
      contentRef.current.scrollTop = contentRef.current.scrollHeight
    }
  }, [displayContent, expanded, isActive])

  if (!hasContent && !isActive) return null

  return (
    <div className="mb-2">
      <button
        type="button"
        className="flex w-full items-center gap-1.5 rounded-md px-1 py-0.5 text-left text-xs text-ink-faint hover:text-ink-soft"
        onClick={() => setExpanded((value) => !value)}
        aria-expanded={expanded}
      >
        <span className="text-[10px]">{expanded ? '▾' : '▸'}</span>
        <span className="font-medium italic">
          {isActive ? t('tool.thinking') : t('tool.thought')}
          {isActive ? <span className="ml-0.5 inline-block animate-pulse">...</span> : null}
        </span>
      </button>
      {expanded && hasContent ? (
        <div
          ref={contentRef}
          className="mt-1 max-h-[280px] overflow-y-auto rounded-lg border border-line bg-paper-hover p-3 text-xs leading-5 text-ink-soft"
        >
          <MarkdownRenderer content={displayContent} variant="compact" streaming={isActive} />
          {isActive ? <span className="ml-px inline-block h-4 w-0.5 animate-pulse bg-ink-faint align-middle" /> : null}
        </div>
      ) : null}
    </div>
  )
}
