import { memo, useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { Check, Copy } from 'lucide-react'
import { useTranslation } from 'react-i18next'

type MarkdownVariant = 'default' | 'document' | 'compact'

interface MarkdownRendererProps {
  content: string
  variant?: MarkdownVariant
  className?: string
  streaming?: boolean
}

type Block =
  | { type: 'heading'; level: number; text: string }
  | { type: 'paragraph'; text: string }
  | { type: 'blockquote'; text: string }
  | { type: 'list'; ordered: boolean; items: string[] }
  | { type: 'code'; language?: string; code: string }
  | { type: 'table'; headers: string[]; rows: string[][] }
  | { type: 'hr' }

interface InlineContext {
  inTable?: boolean
}

const FENCE_RE = /^ {0,3}(`{3,}|~{3,})\s*([\w+-]*)?.*$/
const ORDERED_LIST_RE = /^\s*\d+\.\s+(.+)$/
const UNORDERED_LIST_RE = /^\s*[-*+]\s+(.+)$/
const HEADING_RE = /^(#{1,6})\s+(.+)$/

export const MarkdownRenderer = memo(function MarkdownRenderer({
  content,
  variant = 'default',
  className,
  streaming = false,
}: MarkdownRendererProps) {
  const blocks = useMemo(() => parseMarkdownBlocks(content), [content])
  const classes = getMarkdownClasses(variant, className)

  return (
    <div className={classes}>
      {blocks.map((block, index) => renderBlock(block, index, variant))}
      {streaming ? <span className="ml-0.5 inline-block h-4 w-0.5 animate-pulse bg-clay align-text-bottom" /> : null}
    </div>
  )
})

function parseMarkdownBlocks(content: string): Block[] {
  const lines = content.replace(/\r\n?/g, '\n').split('\n')
  const blocks: Block[] = []
  let index = 0

  while (index < lines.length) {
    const line = lines[index] ?? ''

    if (!line.trim()) {
      index += 1
      continue
    }

    const fence = FENCE_RE.exec(line)
    if (fence) {
      const marker = fence[1]![0]
      const language = fence[2]?.trim() || undefined
      const codeLines: string[] = []
      index += 1

      while (index < lines.length) {
        const current = lines[index] ?? ''
        const closeFence = FENCE_RE.exec(current)
        if (closeFence && closeFence[1]?.[0] === marker) {
          index += 1
          break
        }
        codeLines.push(current)
        index += 1
      }

      blocks.push({ type: 'code', language, code: codeLines.join('\n') })
      continue
    }

    const heading = HEADING_RE.exec(line)
    if (heading) {
      blocks.push({
        type: 'heading',
        level: Math.min(heading[1]!.length, 6),
        text: heading[2]!.trim(),
      })
      index += 1
      continue
    }

    if (/^ {0,3}([-*_])(?:\s*\1){2,}\s*$/.test(line)) {
      blocks.push({ type: 'hr' })
      index += 1
      continue
    }

    if (isTableStart(lines, index)) {
      const headers = splitTableRow(lines[index]!)
      index += 2
      const rows: string[][] = []
      while (index < lines.length && isTableRow(lines[index] ?? '')) {
        rows.push(splitTableRow(lines[index]!))
        index += 1
      }
      blocks.push({ type: 'table', headers, rows })
      continue
    }

    const unordered = UNORDERED_LIST_RE.exec(line)
    const ordered = ORDERED_LIST_RE.exec(line)
    if (unordered || ordered) {
      const items: string[] = []
      const isOrdered = !!ordered
      while (index < lines.length) {
        const current = lines[index] ?? ''
        const match = isOrdered ? ORDERED_LIST_RE.exec(current) : UNORDERED_LIST_RE.exec(current)
        if (!match) break
        items.push(match[1]!.trim())
        index += 1
      }
      blocks.push({ type: 'list', ordered: isOrdered, items })
      continue
    }

    if (line.trimStart().startsWith('>')) {
      const quoteLines: string[] = []
      while (index < lines.length && (lines[index] ?? '').trimStart().startsWith('>')) {
        quoteLines.push((lines[index] ?? '').replace(/^\s*>\s?/, ''))
        index += 1
      }
      blocks.push({ type: 'blockquote', text: quoteLines.join('\n') })
      continue
    }

    const paragraphLines: string[] = []
    while (index < lines.length) {
      const current = lines[index] ?? ''
      if (!current.trim()) break
      if (FENCE_RE.test(current) || HEADING_RE.test(current) || isTableStart(lines, index)) break
      if (UNORDERED_LIST_RE.test(current) || ORDERED_LIST_RE.test(current)) break
      if (current.trimStart().startsWith('>')) break
      paragraphLines.push(current)
      index += 1
    }
    blocks.push({ type: 'paragraph', text: paragraphLines.join('\n') })
  }

  return blocks
}

function renderBlock(block: Block, index: number, variant: MarkdownVariant): ReactNode {
  switch (block.type) {
    case 'heading': {
      return renderHeading(block.level, block.text, index)
    }
    case 'paragraph':
      return <p key={index} className={paragraphClass(variant)}>{renderInline(block.text)}</p>
    case 'blockquote':
      return (
        <blockquote key={index} className={blockquoteClass(variant)}>
          {renderInline(block.text)}
        </blockquote>
      )
    case 'list': {
      const Tag = block.ordered ? 'ol' : 'ul'
      return (
        <Tag key={index} className={listClass(variant, block.ordered)}>
          {block.items.map((item, itemIndex) => (
            <li key={itemIndex} className={listItemClass(variant)}>{renderInline(item)}</li>
          ))}
        </Tag>
      )
    }
    case 'code':
      return <CodeBlock key={index} code={block.code} language={block.language} variant={variant} />
    case 'table':
      return (
        <div key={index} className={tableWrapClass(variant)}>
          <table className={tableClass(block.headers.length)}>
            <colgroup>{renderTableColumns(block.headers.length)}</colgroup>
            <thead>
              <tr>
                {block.headers.map((header, cellIndex) => (
                  <th key={cellIndex} className={tableHeaderCellClass(cellIndex, block.headers.length)}>
                    {renderInline(header, { inTable: true })}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {block.rows.map((row, rowIndex) => (
                <tr key={rowIndex}>
                  {block.headers.map((_, cellIndex) => (
                    <td key={cellIndex} className={tableBodyCellClass(cellIndex, block.headers.length)}>
                      {renderInline(row[cellIndex] ?? '', { inTable: true })}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )
    case 'hr':
      return <hr key={index} className="my-4 border-0 border-t border-line" />
  }
}

function renderHeading(level: number, text: string, key: number): ReactNode {
  switch (level) {
    case 1:
      return <h1 key={key} className={headingClass(1)}>{renderInline(text)}</h1>
    case 2:
      return <h2 key={key} className={headingClass(2)}>{renderInline(text)}</h2>
    case 3:
      return <h3 key={key} className={headingClass(3)}>{renderInline(text)}</h3>
    case 4:
      return <h4 key={key} className={headingClass(4)}>{renderInline(text)}</h4>
    case 5:
      return <h5 key={key} className={headingClass(5)}>{renderInline(text)}</h5>
    default:
      return <h6 key={key} className={headingClass(6)}>{renderInline(text)}</h6>
  }
}

function renderTableColumns(columnCount: number): ReactNode {
  if (columnCount === 2) {
    return (
      <>
        <col className="w-[38%]" />
        <col className="w-[62%]" />
      </>
    )
  }

  return Array.from({ length: columnCount }, (_, index) => <col key={index} />)
}

function renderInline(text: string, context: InlineContext = {}): ReactNode[] {
  const nodes: ReactNode[] = []
  const normalized = text.replace(/  \n/g, '\n')
  const tokenRe = /(`[^`]+`|\*\*[^*]+\*\*|__[^_]+__|\*[^*\n]+\*|_[^_\n]+_|\[[^\]]+\]\([^)]+\)|\n)/g
  let cursor = 0
  let match: RegExpExecArray | null

  while ((match = tokenRe.exec(normalized))) {
    if (match.index > cursor) {
      nodes.push(normalized.slice(cursor, match.index))
    }

    const token = match[0]
    const key = nodes.length
    if (token === '\n') {
      nodes.push(<br key={key} />)
    } else if (token.startsWith('`')) {
      nodes.push(<code key={key} className={inlineCodeClass(context)}>{token.slice(1, -1)}</code>)
    } else if (token.startsWith('**') || token.startsWith('__')) {
      nodes.push(<strong key={key}>{renderInline(token.slice(2, -2), context)}</strong>)
    } else if (token.startsWith('*') || token.startsWith('_')) {
      nodes.push(<em key={key}>{renderInline(token.slice(1, -1), context)}</em>)
    } else if (token.startsWith('[')) {
      const parsed = /^\[([^\]]+)\]\(([^)]+)\)$/.exec(token)
      const label = parsed?.[1] ?? token
      const href = parsed?.[2] ?? ''
      nodes.push(
        isSafeHref(href) ? (
          <a key={key} href={href} target="_blank" rel="noreferrer noopener">
            {renderInline(label, context)}
          </a>
        ) : (
          label
        ),
      )
    }

    cursor = match.index + token.length
  }

  if (cursor < normalized.length) {
    nodes.push(normalized.slice(cursor))
  }

  return nodes
}

function inlineCodeClass(context: InlineContext): string {
  return [
    'rounded bg-code-bg font-mono text-[0.85em] text-ink',
    context.inTable ? 'px-1 py-px leading-5' : 'px-1.5 py-0.5',
  ].join(' ')
}

function CodeBlock({
  code,
  language,
  variant,
}: {
  code: string
  language?: string
  variant: MarkdownVariant
}) {
  const { t } = useTranslation()
  const [copied, setCopied] = useState(false)
  const copiedTimerRef = useRef<number | null>(null)

  useEffect(() => () => {
    if (copiedTimerRef.current !== null) window.clearTimeout(copiedTimerRef.current)
  }, [])

  async function copyCode() {
    try {
      await navigator.clipboard?.writeText(code)
    } catch {
      return
    }
    setCopied(true)
    if (copiedTimerRef.current !== null) window.clearTimeout(copiedTimerRef.current)
    copiedTimerRef.current = window.setTimeout(() => setCopied(false), 1_500)
  }

  return (
    <div className={codeBlockClass(variant)}>
      <div className="flex items-center gap-2 px-3 pt-2">
        <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-paper/45">
          {language || 'text'}
        </span>
        <button
          type="button"
          aria-label={copied ? t('tool.copied') : t('tool.copy')}
          title={copied ? t('tool.copied') : t('tool.copy')}
          onClick={() => void copyCode()}
          className="grid size-6 shrink-0 place-items-center rounded-md text-paper/45 transition-colors hover:bg-paper/10 hover:text-paper"
        >
          {copied ? <Check size={12} className="text-status-success" /> : <Copy size={12} />}
        </button>
      </div>
      <pre className={preClass(variant)}>
        <code className="whitespace-pre border-0 bg-transparent p-0 font-mono text-[0.82rem] leading-relaxed text-paper">
          {code}
        </code>
      </pre>
    </div>
  )
}

function isSafeHref(href: string): boolean {
  return /^(https?:|mailto:|#|\/)/i.test(href)
}

function isTableStart(lines: string[], index: number): boolean {
  const header = lines[index] ?? ''
  const divider = lines[index + 1] ?? ''
  if (!isTableRow(header) || !isTableRow(divider)) return false

  const headers = splitTableRow(header)
  const dividerCells = splitTableRow(divider)

  return (
    headers.length > 0 &&
    headers.length === dividerCells.length &&
    dividerCells.every(isTableDividerCell)
  )
}

function isTableRow(line: string): boolean {
  return line.includes('|') && line.trim().length > 0
}

function isTableDividerCell(cell: string): boolean {
  return /^:?-{3,}:?$/.test(cell.trim())
}

function splitTableRow(line: string): string[] {
  return line
    .trim()
    .replace(/^\|/, '')
    .replace(/\|$/, '')
    .split('|')
    .map((cell) => cell.trim())
}

function tableClass(columnCount: number): string {
  return [
    'w-full table-fixed border-collapse text-sm',
    columnCount > 1 ? 'min-w-[560px]' : 'min-w-[220px]',
  ].join(' ')
}

function getMarkdownClasses(variant: MarkdownVariant, className?: string): string {
  return [
    'min-w-0 max-w-none break-words [overflow-wrap:anywhere] [&>:first-child]:mt-0 [&>:last-child]:mb-0',
    variant === 'compact' ? 'text-xs leading-5 text-ink-soft' : 'text-[0.94rem] leading-7 text-ink',
    '[&_a]:text-clay [&_a]:no-underline hover:[&_a]:underline',
    '[&_strong]:font-semibold [&_strong]:text-ink',
    '[&_li::marker]:text-ink-faint',
    className ?? '',
  ]
    .filter(Boolean)
    .join(' ')
}

function headingClass(level: number): string {
  const size = {
    1: 'mt-5 mb-2 text-lg',
    2: 'mt-4 mb-1.5 text-base',
    3: 'mt-4 mb-1.5 text-sm',
    4: 'mt-3 mb-1 text-sm text-ink-soft',
    5: 'mt-3 mb-1 text-xs text-ink-soft',
    6: 'mt-3 mb-1 text-xs text-ink-soft',
  }[level] ?? 'mt-3 mb-1 text-xs text-ink-soft'

  return `font-semibold leading-snug text-ink ${size}`
}

function paragraphClass(variant: MarkdownVariant): string {
  return variant === 'compact' ? 'my-1 whitespace-normal' : 'my-2 whitespace-normal'
}

function blockquoteClass(variant: MarkdownVariant): string {
  return [
    'border-l-2 border-line-strong text-ink-soft',
    variant === 'compact' ? 'my-1.5 pl-2.5 text-xs' : 'my-2.5 pl-3',
  ].join(' ')
}

function listClass(variant: MarkdownVariant, ordered: boolean): string {
  return [
    ordered ? 'list-decimal' : 'list-disc',
    'list-outside',
    variant === 'compact' ? 'my-1 pl-4' : 'my-2 pl-5',
  ].join(' ')
}

function listItemClass(variant: MarkdownVariant): string {
  return variant === 'compact' ? 'my-0.5 leading-5' : 'my-1'
}

function codeBlockClass(variant: MarkdownVariant): string {
  return [
    'overflow-hidden rounded-lg bg-ink',
    variant === 'compact' ? 'my-2' : 'my-3',
  ].join(' ')
}

function preClass(variant: MarkdownVariant): string {
  return [
    'm-0 overflow-x-auto px-3 pb-2.5 pt-1',
    variant === 'compact' ? 'text-[0.78rem]' : '',
  ].join(' ')
}

function tableWrapClass(variant: MarkdownVariant): string {
  return [
    'overflow-x-auto rounded-lg border border-line',
    variant === 'compact' ? 'my-2' : 'my-3',
  ].join(' ')
}

function tableHeaderCellClass(_cellIndex: number, _columnCount: number): string {
  return 'border-b border-line bg-paper-hover/60 px-3 py-1.5 text-left align-top font-semibold text-ink whitespace-normal break-words [overflow-wrap:anywhere]'
}

function tableBodyCellClass(_cellIndex: number, _columnCount: number): string {
  return 'border-t border-line/70 px-3 py-1.5 align-top text-ink-soft whitespace-normal break-words [overflow-wrap:anywhere]'
}
