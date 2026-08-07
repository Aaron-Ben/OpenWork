import { useEffect, useMemo, useRef, useState } from 'react'
import { ChevronRight } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { MarkdownRenderer } from '@/components/markdown/MarkdownRenderer'

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
    <div className="my-0.5">
      <button
        type="button"
        className="flex items-center gap-1 rounded-md py-0.5 pr-1.5 text-xs text-ink-faint transition-colors hover:text-ink-soft"
        onClick={() => setExpanded((value) => !value)}
        aria-expanded={expanded}
      >
        <ChevronRight
          size={12}
          strokeWidth={2}
          className={`shrink-0 transition-transform duration-200 ${expanded ? 'rotate-90' : ''}`}
        />
        <span className="italic">
          {isActive ? t('tool.thinking') : t('tool.thought')}
        </span>
        {isActive ? (
          <span className="ml-0.5 inline-block animate-pulse" aria-hidden="true">…</span>
        ) : null}
      </button>
      {expanded && hasContent ? (
        <div
          ref={contentRef}
          className="ml-[7px] mt-1 max-h-[260px] overflow-y-auto border-l border-line pl-3"
        >
          {/*
            流式光标由 MarkdownRenderer 在 streaming 时自己画（见其 streaming 分支）。
            这里不要再补一条 —— 之前两处各画一条，界面上就是一橘一灰两根竖线。
          */}
          <MarkdownRenderer content={displayContent} variant="compact" streaming={isActive} />
        </div>
      ) : null}
    </div>
  )
}
