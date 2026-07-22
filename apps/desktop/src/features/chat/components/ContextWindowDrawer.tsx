import { Fragment, useEffect, useRef } from 'react'
import { ChevronRight, Layers3, MessageSquareText, Paperclip, RefreshCw, Wrench, X } from 'lucide-react'
import { motion } from 'motion/react'
import { useTranslation } from 'react-i18next'

import type {
  RuntimeContextInspectionMessage,
  RuntimeContextWindowInspection,
} from '../../../bridge/compat'
import { MarkdownRenderer } from '../../../components/markdown/MarkdownRenderer'
import type { ContentBlock } from '../../../type/parts'
import { extractText } from '../../../type/parts'
import { formatTokenCount } from './ContextUsageIndicator'

interface ContextWindowDrawerProps {
  inspection: RuntimeContextWindowInspection | null
  contextWindowTokens: number
  highlightedTurnId?: string | null
  loading: boolean
  error: string | null
  onRefresh: () => void
  onClose: () => void
}

export function ContextWindowDrawer({
  inspection,
  contextWindowTokens,
  highlightedTurnId,
  loading,
  error,
  onRefresh,
  onClose,
}: ContextWindowDrawerProps) {
  const { t } = useTranslation()
  const drawerRef = useRef<HTMLElement>(null)
  const previousFocus = useRef<HTMLElement | null>(null)

  useEffect(() => {
    previousFocus.current = document.activeElement as HTMLElement | null
    drawerRef.current?.focus()
    return () => previousFocus.current?.focus()
  }, [])

  const currentTurnId = highlightedTurnId ?? inspection?.currentTurnId ?? null
  const usedTokens = inspection?.budget.estimatedInputTokens ?? 0
  const usedPercent = contextWindowTokens > 0
    ? Math.min(100, Math.max(0, Math.round((usedTokens / contextWindowTokens) * 100)))
    : 0

  return (
    <motion.div
      className="fixed inset-0 z-40 bg-ink/10 backdrop-blur-[1px]"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      onMouseDown={(event) => { if (event.target === event.currentTarget) onClose() }}
    >
      <motion.aside
        ref={drawerRef}
        role="dialog"
        aria-modal="true"
        aria-label={t('chat.contextInspector.title')}
        tabIndex={-1}
        initial={{ opacity: 0, x: 32 }}
        animate={{ opacity: 1, x: 0 }}
        className="absolute inset-y-0 right-0 flex w-[min(680px,96vw)] flex-col border-l border-line bg-paper shadow-[-18px_0_45px_rgba(20,20,19,0.12)] outline-none max-[640px]:w-full"
        onKeyDown={(event) => { if (event.key === 'Escape') onClose() }}
      >
        <header className="border-b border-line px-5 py-4">
          <div className="flex items-start gap-3">
            <div className="min-w-0 flex-1">
              <h2 className="text-base font-semibold text-ink">
                {t('chat.contextInspector.title')}
              </h2>
              <p className="mt-1 text-xs leading-5 text-ink-faint">
                {t('chat.contextInspector.previewDescription')}
              </p>
            </div>
            <button
              type="button"
              onClick={onRefresh}
              disabled={loading}
              className="rounded-lg p-2 text-ink-soft hover:bg-paper-hover disabled:opacity-45"
              aria-label={t('chat.contextInspector.refresh')}
            >
              <RefreshCw size={17} className={loading ? 'animate-spin' : undefined} />
            </button>
            <button
              type="button"
              onClick={onClose}
              className="rounded-lg p-2 text-ink-soft hover:bg-paper-hover"
              aria-label={t('chat.contextInspector.close')}
            >
              <X size={18} />
            </button>
          </div>

          {inspection ? (
            <div className="mt-4 grid grid-cols-2 gap-2 min-[520px]:grid-cols-4">
              <BudgetStat
                label={t('chat.contextInspector.used')}
                value={`${formatTokenCount(usedTokens)} / ${formatTokenCount(contextWindowTokens)}`}
              />
              <BudgetStat label={t('chat.contextInspector.usage')} value={`${usedPercent}%`} />
              <BudgetStat
                label={t('chat.contextInspector.messages')}
                value={String(inspection.conversation.length)}
              />
              <BudgetStat
                label={t('chat.contextInspector.tools')}
                value={String(inspection.toolSurface.length)}
              />
            </div>
          ) : null}
        </header>

        <div className="min-h-0 flex-1 overflow-auto px-5 py-5">
          {loading && !inspection ? <ContextLoading /> : null}
          {error ? (
            <p className="rounded-xl bg-status-danger-soft p-3 text-sm text-status-danger-ink" role="alert">
              {error}
            </p>
          ) : null}
          {inspection ? (
            <div className="grid gap-5">
              <ContextSection
                icon={<Layers3 size={16} />}
                title={t('chat.contextInspector.systemContext')}
                description={t('chat.contextInspector.systemDescription')}
                tokens={inspection.budget.systemContextTokens}
              >
                <div className="grid gap-2">
                  {inspection.systemContext.map((part, index) => (
                    <details
                      key={part.sourceKey}
                      open={index === 0}
                      className="rounded-xl border border-line bg-surface/40 px-3 py-2.5"
                    >
                      <summary className="cursor-pointer font-mono text-xs text-ink-soft">
                        {part.sourceKey}
                      </summary>
                      <ContentPreview content={part.content} />
                    </details>
                  ))}
                </div>
              </ContextSection>

              <ContextSection
                icon={<MessageSquareText size={16} />}
                title={t('chat.contextInspector.conversation')}
                description={t('chat.contextInspector.conversationDescription')}
                tokens={inspection.budget.conversationTokens}
              >
                {inspection.conversation.length > 0 ? (
                  <div className="grid gap-2">
                    {inspection.conversation.map((message, index) => {
                      const previousTurnId = inspection.conversation[index - 1]?.turnId
                      const showTurnDivider = index > 0 && message.turnId !== previousTurnId
                      return (
                        <Fragment key={message.messageId}>
                          {showTurnDivider ? (
                            <div className="my-3 border-t-2 border-line" role="separator" />
                          ) : null}
                          <ConversationEntry
                            message={message}
                            isCurrentTurn={Boolean(currentTurnId && message.turnId === currentTurnId)}
                          />
                        </Fragment>
                      )
                    })}
                  </div>
                ) : (
                  <EmptySection label={t('chat.contextInspector.noConversation')} />
                )}
              </ContextSection>

              <ContextSection
                icon={<Wrench size={16} />}
                title={t('chat.contextInspector.toolSurface')}
                description={t('chat.contextInspector.toolDescription')}
                tokens={inspection.budget.toolSurfaceTokens}
              >
                {inspection.toolSurface.length > 0 ? (
                  <div className="grid gap-2">
                    {inspection.toolSurface.map((tool) => (
                      <details key={tool.name} className="rounded-xl border border-line px-3 py-2.5">
                        <summary className="cursor-pointer text-sm font-medium text-ink">
                          {tool.name}
                        </summary>
                        <p className="mt-2 whitespace-pre-wrap text-xs leading-5 text-ink-soft">
                          {tool.description}
                        </p>
                        <pre className="mt-2 overflow-auto rounded-lg bg-surface p-3 font-mono text-[11px] leading-5 text-ink-soft">
                          {JSON.stringify(tool.parameters, null, 2)}
                        </pre>
                      </details>
                    ))}
                  </div>
                ) : (
                  <EmptySection label={t('chat.contextInspector.noTools')} />
                )}
              </ContextSection>

              <p className="rounded-xl border border-line bg-surface/50 px-3 py-2.5 text-[11px] leading-5 text-ink-faint">
                {t('chat.contextInspector.providerOverheadNote')}
              </p>
            </div>
          ) : null}
        </div>
      </motion.aside>
    </motion.div>
  )
}

function BudgetStat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg bg-surface px-2.5 py-2">
      <div className="text-[10px] text-ink-faint">{label}</div>
      <div className="mt-0.5 truncate font-mono text-xs tabular-nums text-ink">{value}</div>
    </div>
  )
}

function ContextSection({
  icon,
  title,
  description,
  tokens,
  children,
}: {
  icon: React.ReactNode
  title: string
  description: string
  tokens: number
  children: React.ReactNode
}) {
  const { t } = useTranslation()
  return (
    <section>
      <div className="mb-2.5 flex items-start gap-2.5">
        <span className="mt-0.5 grid size-7 shrink-0 place-items-center rounded-lg bg-paper-hover text-ink-soft">
          {icon}
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex items-center justify-between gap-3">
            <h3 className="text-sm font-semibold text-ink">{title}</h3>
            <span className="shrink-0 font-mono text-[11px] text-ink-faint">
              {t('chat.contextInspector.estimatedTokens', { count: formatTokenCount(tokens) })}
            </span>
          </div>
          <p className="mt-0.5 text-[11px] leading-5 text-ink-faint">{description}</p>
        </div>
      </div>
      {children}
    </section>
  )
}

function ConversationEntry({
  message,
  isCurrentTurn,
}: {
  message: RuntimeContextInspectionMessage
  isCurrentTurn: boolean
}) {
  const { t } = useTranslation()
  return (
    <details
      className={`group rounded-xl border px-3 py-2.5 ${isCurrentTurn ? 'border-clay/55 bg-clay/5' : 'border-line'}`}
      data-current-turn={isCurrentTurn ? 'true' : undefined}
    >
      <summary className="flex cursor-pointer list-none flex-wrap items-center gap-2 [&::-webkit-details-marker]:hidden">
        <ChevronRight
          size={14}
          className="shrink-0 text-ink-faint transition-transform group-open:rotate-90"
        />
        <span className="rounded-full bg-surface px-2 py-0.5 text-[10px] font-medium uppercase tracking-wide text-ink-soft">
          {t(`chat.contextInspector.roles.${message.role}`)}
        </span>
        {isCurrentTurn ? (
          <span className="rounded-full bg-clay-soft px-2 py-0.5 text-[10px] font-medium text-clay">
            {t('chat.contextInspector.currentTurn')}
          </span>
        ) : null}
        <span className="ml-auto truncate font-mono text-[10px] text-ink-faint" title={message.turnId ?? message.messageId}>
          {message.turnId ?? message.messageId}
        </span>
      </summary>
      <ContentPreview content={message.content} />
    </details>
  )
}

function ContentPreview({ content }: { content: ContentBlock[] }) {
  if (content.length === 0) return null
  return (
    <div className="mt-2 grid gap-2">
      {content.map((block, index) => (
        <ContentBlockView key={index} block={block} />
      ))}
    </div>
  )
}

type BlockTone = 'neutral' | 'success' | 'warning' | 'danger'

const blockToneClasses: Record<BlockTone, string> = {
  neutral: 'bg-surface text-ink-faint',
  success: 'bg-status-success-soft text-status-success-ink',
  warning: 'bg-status-warning-soft text-status-warning-ink',
  danger: 'bg-status-danger-soft text-status-danger-ink',
}

function LabeledBlock({
  label,
  title,
  state,
  tone = 'neutral',
  children,
}: {
  label: string
  title?: string
  state?: string
  tone?: BlockTone
  children: React.ReactNode
}) {
  return (
    <div className="overflow-hidden rounded-lg border border-line">
      <div className="flex items-center gap-2 border-b border-line bg-surface/60 px-3 py-1.5">
        <span className="shrink-0 text-[10px] font-medium uppercase tracking-wide text-ink-faint">{label}</span>
        {title ? <span className="truncate font-mono text-[11px] text-ink-soft">{title}</span> : null}
        {state ? (
          <span className={`ml-auto shrink-0 rounded-full px-2 py-0.5 text-[10px] font-medium ${blockToneClasses[tone]}`}>
            {state}
          </span>
        ) : null}
      </div>
      <div className="max-h-80 overflow-auto p-3">{children}</div>
    </div>
  )
}

function tryParseJsonPayload(raw: string): unknown | null {
  const trimmed = raw.trim()
  if (!trimmed.startsWith('{') && !trimmed.startsWith('[')) return null
  try {
    return JSON.parse(trimmed)
  } catch {
    return null
  }
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

const PAYLOAD_PATH_KEYS = ['path', 'file_path', 'filePath', 'filename']

function inferPayloadLanguage(payload: Record<string, unknown>): string | undefined {
  for (const key of PAYLOAD_PATH_KEYS) {
    const value = payload[key]
    if (typeof value === 'string') {
      const match = /\.([A-Za-z0-9]+)$/.exec(value.trim())
      if (match) return match[1]
    }
  }
  return undefined
}

function ToolPayloadView({ raw }: { raw: string }) {
  const payload = tryParseJsonPayload(raw)
  if (payload === null) {
    return (
      <pre className="overflow-auto whitespace-pre-wrap break-words font-mono text-[11px] leading-5 text-ink-soft">
        {raw}
      </pre>
    )
  }
  if (isPlainObject(payload)) {
    const languageHint = inferPayloadLanguage(payload)
    return (
      <div className="grid gap-2">
        {Object.entries(payload).map(([key, value]) => (
          <PayloadEntry key={key} name={key} value={value} languageHint={languageHint} />
        ))}
      </div>
    )
  }
  return (
    <MarkdownRenderer
      variant="compact"
      content={`\`\`\`json\n${JSON.stringify(payload, null, 2)}\n\`\`\``}
    />
  )
}

function PayloadEntry({
  name,
  value,
  languageHint,
}: {
  name: string
  value: unknown
  languageHint?: string
}) {
  const isString = typeof value === 'string'
  const text = isString ? (value as string) : (JSON.stringify(value, null, 2) ?? String(value))
  const useCodeBlock = text.includes('\n') || text.length > 120
  return (
    <div>
      <div className="mb-1 font-mono text-[10px] font-medium uppercase tracking-wide text-ink-faint">
        {name}
      </div>
      {useCodeBlock ? (
        <MarkdownRenderer
          variant="compact"
          content={`\`\`\`${isString ? (languageHint ?? '') : 'json'}\n${text}\n\`\`\``}
        />
      ) : (
        <code className="break-all font-mono text-[11px] leading-5 text-ink-soft">{text}</code>
      )}
    </div>
  )
}

function ContentBlockView({ block }: { block: ContentBlock }) {
  const { t } = useTranslation()
  switch (block.type) {
    case 'text':
      return (
        <div className="max-h-80 overflow-auto whitespace-pre-wrap break-words rounded-lg bg-surface p-3 text-xs leading-5 text-ink-soft">
          {block.text}
        </div>
      )
    case 'thinking':
      return (
        <LabeledBlock label={t('chat.contextInspector.blocks.thinking')}>
          <p className="whitespace-pre-wrap break-words text-[11px] leading-5 text-ink-faint">{block.thinking}</p>
        </LabeledBlock>
      )
    case 'tool_call':
      return (
        <LabeledBlock
          label={t('chat.contextInspector.blocks.toolCall')}
          title={block.name}
          state={block.state}
          tone={block.state === 'finished' ? 'success' : 'warning'}
        >
          <ToolPayloadView raw={block.input} />
        </LabeledBlock>
      )
    case 'tool_result': {
      const tone: BlockTone =
        block.state === 'success'
          ? 'success'
          : block.state === 'error' || block.state === 'denied'
            ? 'danger'
            : 'warning'
      const textOutput = extractText(block.output)
      const richOutput = block.output.filter((child) => child.type !== 'text')
      return (
        <LabeledBlock
          label={t('chat.contextInspector.blocks.toolResult')}
          title={block.name}
          state={block.state}
          tone={tone}
        >
          {textOutput ? <ToolPayloadView raw={textOutput} /> : null}
          {richOutput.length > 0 ? (
            <div className={`grid gap-2 ${textOutput ? 'mt-2' : ''}`}>
              {richOutput.map((child, index) => (
                <ContentBlockView key={index} block={child} />
              ))}
            </div>
          ) : null}
        </LabeledBlock>
      )
    }
    case 'data': {
      const detail =
        block.source.source_type === 'url'
          ? block.source.url
          : block.source.source_type === 'file_id'
            ? block.source.id
            : t('chat.contextInspector.blocks.base64Data', { count: block.source.data.length })
      return (
        <div className="flex items-center gap-2 rounded-lg border border-line bg-surface/60 px-3 py-2">
          <Paperclip size={12} className="shrink-0 text-ink-faint" />
          <span className="shrink-0 rounded-full bg-surface px-2 py-0.5 font-mono text-[10px] text-ink-faint">
            {block.source.source_type === 'file_id' ? 'file_id' : block.source.media_type}
          </span>
          <span className="truncate font-mono text-[11px] text-ink-soft" title={detail}>
            {block.name ?? detail}
          </span>
        </div>
      )
    }
    case 'provider_opaque':
      return (
        <LabeledBlock label={t('chat.contextInspector.blocks.providerPayload')} title={block.kind}>
          <pre className="overflow-auto whitespace-pre-wrap break-words font-mono text-[11px] leading-5 text-ink-soft">
            {JSON.stringify(block.payload, null, 2)}
          </pre>
        </LabeledBlock>
      )
  }
}

function EmptySection({ label }: { label: string }) {
  return <p className="rounded-xl border border-dashed border-line px-3 py-4 text-center text-xs text-ink-faint">{label}</p>
}

function ContextLoading() {
  const { t } = useTranslation()
  return (
    <div className="grid gap-3" aria-label={t('chat.contextInspector.loading')}>
      {[0, 1, 2].map((item) => <div key={item} className="h-28 animate-pulse rounded-xl bg-surface" />)}
    </div>
  )
}
