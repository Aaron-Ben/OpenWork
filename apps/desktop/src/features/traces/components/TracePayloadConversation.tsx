import { Bot, FileText, Image as ImageIcon, MessageSquare, User, Wrench } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { extractText, type ContentBlock, type DataBlock } from '@/types/parts'

export interface TracePayloadMessage {
  role: string
  content: ContentBlock[]
}

const TEXT_PREVIEW_CHARS = 600
const TOOL_INPUT_PREVIEW_CHARS = 300

/**
 * 解析 request 槽位的正文：openwork-models `Message` 数组的 serde 形态
 * （`[{ role, content: ContentBlock[] }]`）。形状不符返回 null，调用方回退 JSON 视图。
 */
export function parseTracePayloadMessages(body: unknown): TracePayloadMessage[] | null {
  if (!Array.isArray(body) || body.length === 0) return null
  const messages: TracePayloadMessage[] = []
  for (const item of body) {
    if (typeof item !== 'object' || item === null) return null
    const candidate = item as { role?: unknown; content?: unknown }
    if (typeof candidate.role !== 'string' || !Array.isArray(candidate.content)) return null
    messages.push({ role: candidate.role, content: candidate.content as ContentBlock[] })
  }
  return messages
}

/** 请求正文的消息视图：按 role 分卡，长块默认收起。
 *  内联（抽屉详情列）时整区限高滚动；relaxed（模态窗）时由外层容器接管滚动。 */
export function TracePayloadConversation({
  messages,
  relaxed = false,
}: {
  messages: TracePayloadMessage[]
  relaxed?: boolean
}) {
  return (
    <div className={`grid content-start gap-2 ${relaxed ? '' : 'max-h-[32rem] overflow-auto pr-1'}`}>
      {messages.map((message, index) => (
        <PayloadMessageCard key={index} message={message} index={index} />
      ))}
    </div>
  )
}

function PayloadMessageCard({ message, index }: { message: TracePayloadMessage; index: number }) {
  const { t } = useTranslation()
  return (
    <article
      data-payload-message={index}
      data-payload-role={message.role}
      className="rounded-lg border border-line bg-paper"
    >
      <header className="flex items-center gap-1.5 border-b border-line/70 px-2.5 py-1.5">
        <PayloadRoleIcon role={message.role} />
        <span className="text-[11px] font-semibold text-ink-soft">
          {t(`chat.contextInspector.roles.${message.role}`, { defaultValue: message.role })}
        </span>
        <span className="ml-auto text-[10px] tabular-nums text-ink-faint">#{index + 1}</span>
      </header>
      <div className="grid gap-2 px-2.5 py-2">
        {message.content.map((block, blockIndex) => (
          <PayloadBlockView key={blockIndex} block={block} />
        ))}
      </div>
    </article>
  )
}

function PayloadRoleIcon({ role }: { role: string }) {
  const className = 'shrink-0 text-ink-faint'
  if (role === 'user') return <User size={12} className={className} />
  if (role === 'assistant') return <Bot size={12} className={className} />
  if (role === 'tool') return <Wrench size={12} className={className} />
  if (role === 'system') return <FileText size={12} className={className} />
  return <MessageSquare size={12} className={className} />
}

function PayloadBlockView({ block }: { block: ContentBlock }) {
  const { t } = useTranslation()
  switch (block.type) {
    case 'text':
      return <CollapsiblePayloadText text={block.text} />
    case 'thinking':
      return (
        <div>
          <PayloadBlockLabel label={t('chat.contextInspector.blocks.thinking')} />
          <CollapsiblePayloadText text={block.thinking} muted />
        </div>
      )
    case 'tool_call':
      return (
        <div className="rounded-md bg-surface px-2 py-1.5">
          <div className="flex items-center gap-1.5 text-[11px] font-medium text-ink-soft">
            <Wrench size={11} className="shrink-0 text-trace-bar-tool" />
            <span className="font-mono">{block.name}</span>
          </div>
          {block.input ? (
            <CollapsiblePayloadText text={block.input} mono previewChars={TOOL_INPUT_PREVIEW_CHARS} />
          ) : null}
        </div>
      )
    case 'tool_result': {
      const text = extractText(block.output)
      return (
        <div className="rounded-md bg-surface px-2 py-1.5">
          <div className="flex items-center gap-1.5 text-[11px] font-medium text-ink-soft">
            <PayloadBlockLabel label={t('chat.contextInspector.blocks.toolResult')} />
            <span className="font-mono">{block.name}</span>
          </div>
          {text
            ? <CollapsiblePayloadText text={text} mono />
            : <p className="text-[11px] text-ink-faint">—</p>}
        </div>
      )
    }
    case 'data':
      return <PayloadDataBlockView block={block} />
    case 'provider_opaque':
      return (
        <div>
          <PayloadBlockLabel label={`${t('chat.contextInspector.blocks.providerPayload')} · ${block.kind}`} />
          <CollapsiblePayloadText text={safeStringify(block.payload)} mono previewChars={TOOL_INPUT_PREVIEW_CHARS} />
        </div>
      )
    default:
      // 前向兼容：未识别的块类型退化为截断 JSON，而不是渲染失败。
      return <CollapsiblePayloadText text={safeStringify(block)} mono previewChars={TOOL_INPUT_PREVIEW_CHARS} />
  }
}

function PayloadBlockLabel({ label }: { label: string }) {
  return (
    <span className="text-[10px] font-semibold uppercase tracking-wide text-ink-faint">
      {label}
    </span>
  )
}

function PayloadDataBlockView({ block }: { block: DataBlock }) {
  const { t } = useTranslation()
  const source = block.source
  const description = source.source_type === 'base64'
    ? `${source.media_type} · ${t('chat.contextInspector.blocks.base64Data', { count: source.data.length })}`
    : source.source_type === 'url'
      ? `${source.media_type} · ${source.url}`
      : source.id
  return (
    <div className="flex items-center gap-1.5 text-[11px] text-ink-faint">
      <ImageIcon size={12} className="shrink-0" />
      {block.name ? <span className="shrink-0 font-medium text-ink-soft">{block.name}</span> : null}
      <span className="truncate">{description}</span>
    </div>
  )
}

function CollapsiblePayloadText({
  text,
  mono = false,
  muted = false,
  previewChars = TEXT_PREVIEW_CHARS,
}: {
  text: string
  mono?: boolean
  muted?: boolean
  previewChars?: number
}) {
  const { t } = useTranslation()
  const [expanded, setExpanded] = useState(false)
  const truncated = text.length > previewChars
  const shown = expanded || !truncated ? text : text.slice(0, previewChars)
  return (
    <div>
      <p
        className={`whitespace-pre-wrap text-[11px] leading-5 ${mono ? 'break-all font-mono' : 'break-words'} ${
          muted ? 'text-ink-faint' : 'text-ink-soft'
        }`}
      >
        {shown}{truncated && !expanded ? ' …' : ''}
      </p>
      {truncated ? (
        <button
          type="button"
          onClick={() => setExpanded((value) => !value)}
          className="mt-0.5 text-[11px] font-medium text-clay hover:underline"
        >
          {expanded ? t('activity.payloads.showPreview') : t('activity.payloads.showAll')}
        </button>
      ) : null}
    </div>
  )
}

function safeStringify(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2) ?? String(value)
  } catch {
    return String(value)
  }
}
