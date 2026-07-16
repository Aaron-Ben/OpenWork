import { AlertTriangle, MessageSquareText } from 'lucide-react'
import { useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

import type { SessionMessage } from '../../type/session'
import type { TraceSpanDetailView } from '../../type/trace'
import { formatDuration } from './traceViewModel'

export type TraceDetailTab = 'overview' | 'input_output' | 'metadata'

export function getTraceDetailTabs(value: TraceSpanDetailView): TraceDetailTab[] {
  if (['step', 'model_attempt', 'tool_run'].includes(value.detail.kind)) {
    return ['overview', 'input_output', 'metadata']
  }
  return ['overview', 'metadata']
}

export function TraceSpanDetailPanel({
  value,
  onRevealMessage,
  onRevealTool,
  initialTab = 'overview',
}: {
  value: TraceSpanDetailView
  onRevealMessage?: (messageId: string) => void
  onRevealTool?: (providerToolCallId: string) => void
  initialTab?: TraceDetailTab
}) {
  const { i18n, t } = useTranslation()
  const { span } = value
  const tabs = getTraceDetailTabs(value)
  const [activeTab, setActiveTab] = useState<TraceDetailTab>(
    tabs.includes(initialTab) ? initialTab : 'overview',
  )

  return (
    <section data-trace-span-detail={span.spanId} className="min-w-0 p-4">
      <div className="flex items-start justify-between gap-3 border-b border-line pb-3">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-[10px] uppercase tracking-wide text-ink-faint">
              {t(`trace.kind.${span.spanKind}`)}
            </span>
            <span className={`rounded-md px-1.5 py-0.5 text-[10px] font-medium ${spanStatusStyle(span.status)}`}>
              {t(`settings.trace.status.${span.status}`)}
            </span>
          </div>
          <h3 className="mt-1 truncate font-sans text-base font-semibold text-ink">
            {span.spanName}
          </h3>
        </div>
        <span className="shrink-0 rounded-md bg-paper-hover px-2 py-1 text-xs text-ink-soft">
          {span.durationMs == null ? '—' : formatDuration(span.durationMs, i18n.language)}
        </span>
      </div>

      {span.errorMessage ? (
        <div className="mt-4 rounded-xl border border-status-danger-border bg-status-danger-soft p-3 text-status-danger-ink">
          <div className="mb-2 flex items-center gap-2 text-xs font-semibold">
            <AlertTriangle size={14} />
            {span.errorCode ?? span.errorType ?? t('trace.errors')}
          </div>
          <pre className="max-h-36 overflow-auto whitespace-pre-wrap font-mono text-xs">
            {span.errorMessage}
          </pre>
        </div>
      ) : null}

      <div
        role="tablist"
        aria-label={t('trace.detail.tabs.label')}
        data-trace-detail-tabs="true"
        className="mt-4 flex gap-1 overflow-x-auto border-b border-line"
      >
        {tabs.map((tab) => (
          <button
            key={tab}
            type="button"
            role="tab"
            data-trace-detail-tab-button={tab}
            aria-selected={activeTab === tab}
            aria-controls={`trace-detail-panel-${span.spanId}-${tab}`}
            id={`trace-detail-tab-${span.spanId}-${tab}`}
            onClick={() => setActiveTab(tab)}
            className={`shrink-0 border-b-2 px-3 py-2 text-xs font-medium transition-colors ${
              activeTab === tab
                ? 'border-clay text-ink'
                : 'border-transparent text-ink-faint hover:text-ink'
            }`}
          >
            {t(`trace.detail.tabs.${tab}`)}
          </button>
        ))}
      </div>

      <div
        role="tabpanel"
        id={`trace-detail-panel-${span.spanId}-${activeTab}`}
        aria-labelledby={`trace-detail-tab-${span.spanId}-${activeTab}`}
        data-trace-detail-panel={activeTab}
        className="mt-4 space-y-4"
      >
        {activeTab === 'overview' ? <OverviewPanel value={value} /> : null}
        {activeTab === 'input_output' ? (
          <InputOutputPanel
            value={value}
            onRevealMessage={onRevealMessage}
            onRevealTool={onRevealTool}
          />
        ) : null}
        {activeTab === 'metadata' ? <MetadataPanel value={value} /> : null}
      </div>
    </section>
  )
}

function OverviewPanel({ value }: { value: TraceSpanDetailView }) {
  const { i18n, t } = useTranslation()
  const { detail } = value

  if (detail.kind === 'turn') {
    return (
      <DetailGrid
        fields={[
          [t('trace.detail.outcome'), detail.data.outcome],
          [t('trace.detail.completeness'), t(`trace.completeness.${detail.data.dataCompleteness}`)],
          [t('trace.detail.diagnosis'), t(`trace.diagnosis.${detail.data.diagnosis.reason}`)],
        ]}
      />
    )
  }

  if (detail.kind === 'step') {
    return (
      <DetailGrid
        fields={[
          [t('trace.detail.stepIndex'), detail.data.stepIndex],
          [t('trace.detail.toolCount'), detail.data.toolCount],
          [t('trace.detail.messageCount'), detail.data.messages.length],
        ]}
      />
    )
  }

  if (detail.kind === 'model_attempt') {
    return (
      <>
        <DetailGrid
          fields={[
            [t('trace.detail.provider'), detail.data.providerId],
            [t('trace.detail.model'), detail.data.model],
            [t('trace.detail.finishReason'), detail.data.finishReason],
            [t('trace.detail.rawFinishReason'), detail.data.rawFinishReason],
          ]}
        />
        <div className="grid grid-cols-3 gap-2 max-[620px]:grid-cols-2">
          <MetricCard label={t('trace.detail.inputTokens')} value={detail.data.usage.inputTokens} />
          <MetricCard label={t('trace.detail.outputTokens')} value={detail.data.usage.outputTokens} />
          <MetricCard label={t('trace.detail.totalTokens')} value={detail.data.usage.totalTokens} />
          <MetricCard label={t('trace.detail.cachedTokens')} value={detail.data.usage.cachedInputTokens} />
          <MetricCard label={t('trace.detail.reasoningTokens')} value={detail.data.usage.reasoningTokens} />
          <MetricCard
            label={t('trace.detail.firstOutput')}
            value={duration(detail.data.firstOutputMs, i18n.language) ?? '—'}
          />
        </div>
        <RequestShapeSummary value={value} />
      </>
    )
  }

  if (detail.kind === 'transport_attempt') {
    return (
      <DetailGrid
        fields={[
          [t('trace.detail.provider'), detail.data.providerId],
          [t('trace.detail.attempt'), detail.data.transportAttempt],
          [t('trace.detail.httpStatus'), detail.data.httpStatus],
          [t('trace.detail.providerCode'), detail.data.providerCode],
          [t('trace.detail.retryDelay'), duration(detail.data.retryDelayMs, i18n.language)],
          [t('trace.detail.willRetry'), booleanText(detail.data.willRetry, t)],
          [t('trace.detail.failurePhase'), detail.data.failurePhase],
          [t('trace.detail.deliveryState'), detail.data.deliveryState],
        ]}
      />
    )
  }

  if (detail.kind === 'tool_run') {
    return (
      <>
        <DetailGrid
          fields={[
            [t('trace.detail.tool'), detail.data.toolName],
            [t('trace.detail.approvalRequired'), booleanText(detail.data.approvalRequired, t)],
          ]}
        />
        <div className="grid grid-cols-3 gap-2 max-[620px]:grid-cols-1">
          <TimingCard label={t('trace.detail.requestToEnd')} value={detail.data.requestToEndMs} />
          <TimingCard label={t('trace.detail.approvalWait')} value={detail.data.approvalWaitMs} />
          <TimingCard label={t('trace.detail.execution')} value={detail.data.executionMs} />
        </div>
      </>
    )
  }

  if (detail.kind === 'approval') {
    return (
      <DetailGrid
        fields={[
          [t('trace.detail.reason'), detail.data.reason],
          [t('trace.detail.tool'), detail.data.toolName],
          [t('trace.detail.resolution'), detail.data.resolution],
          [t('trace.detail.wait'), duration(detail.data.waitMs, i18n.language)],
        ]}
      />
    )
  }

  return (
    <DetailGrid
      fields={[
        [t('trace.detail.reason'), detail.data.reason],
        [t('trace.detail.stepIndex'), detail.data.stepIndex],
      ]}
    />
  )
}

function InputOutputPanel({
  value,
  onRevealMessage,
  onRevealTool,
}: {
  value: TraceSpanDetailView
  onRevealMessage?: (messageId: string) => void
  onRevealTool?: (providerToolCallId: string) => void
}) {
  const { t } = useTranslation()
  const { detail } = value

  if (detail.kind === 'step') {
    return <MessageList messages={detail.data.messages} onRevealMessage={onRevealMessage} />
  }

  if (detail.kind === 'model_attempt') {
    return (
      <>
        <div className="rounded-xl border border-line bg-paper-hover p-3 text-xs leading-5 text-ink-faint">
          {t('trace.detail.contextDisclaimer')}
        </div>
        <MessageList messages={detail.data.messages} onRevealMessage={onRevealMessage} />
      </>
    )
  }

  if (detail.kind === 'tool_run') {
    return (
      <>
        {detail.data.providerToolCallId && onRevealTool ? (
          <button
            type="button"
            className="text-xs text-clay hover:underline"
            onClick={() => onRevealTool(detail.data.providerToolCallId!)}
          >
            {t('trace.detail.revealTool')}
          </button>
        ) : null}
        <JsonBlock
          label={t('trace.detail.input')}
          value={detail.data.input}
          splitStringContent
        />
        <JsonBlock label={t('trace.detail.observation')} value={detail.data.observation} />
      </>
    )
  }

  return null
}

function MetadataPanel({ value }: { value: TraceSpanDetailView }) {
  const { i18n, t } = useTranslation()
  const { detail, span } = value

  return (
    <>
      <DetailSection title={t('trace.detail.identifiers')}>
        <DetailGrid
          fields={[
            [t('trace.detail.traceId'), span.traceId],
            [t('trace.detail.spanId'), span.spanId],
            [t('trace.detail.parentSpanId'), span.parentSpanId],
            [t('trace.detail.sessionId'), span.sessionId],
            [t('trace.detail.turnId'), span.turnId],
            [t('trace.detail.stepId'), span.stepId],
            [t('trace.detail.toolRunId'), span.toolRunId],
          ]}
        />
      </DetailSection>

      <DetailSection title={t('trace.detail.timing')}>
        <DetailGrid
          fields={[
            [t('trace.detail.startedAt'), formatTimestamp(span.startedAt, i18n.language)],
            [t('trace.detail.endedAt'), formatTimestamp(span.endedAt, i18n.language)],
            [t('trace.duration'), duration(span.durationMs, i18n.language)],
          ]}
        />
      </DetailSection>

      <DetailGrid
        fields={[
          [t('trace.detail.errorType'), span.errorType],
          [t('trace.detail.errorCode'), span.errorCode],
        ]}
      />

      {detail.kind === 'turn' ? (
        <DetailGrid
          fields={[
            [t('trace.detail.schemaVersion'), detail.data.traceSchemaVersion],
            [t('trace.detail.instrumentationVersion'), detail.data.instrumentationVersion],
            [t('trace.detail.appVersion'), detail.data.appVersion],
            [t('trace.detail.captureMode'), detail.data.captureMode],
          ]}
        />
      ) : null}

      {detail.kind === 'model_attempt' ? (
        <DetailGrid
          fields={[
            [t('trace.detail.responseId'), detail.data.responseId],
            [t('trace.detail.providerRequestId'), detail.data.providerRequestId],
          ]}
        />
      ) : null}

      {detail.kind === 'transport_attempt' ? (
        <DetailGrid
          fields={[
            [t('trace.detail.providerRequestId'), detail.data.providerRequestId],
          ]}
        />
      ) : null}

      {detail.kind === 'tool_run' ? (
        <DetailGrid
          fields={[
            [t('trace.detail.providerToolCallId'), detail.data.providerToolCallId],
            [t('trace.detail.requestedAt'), formatTimestamp(detail.data.requestedAt, i18n.language)],
            [t('trace.detail.executionStartedAt'), formatTimestamp(detail.data.executionStartedAt, i18n.language)],
          ]}
        />
      ) : null}

      {detail.kind === 'recovery' ? (
        <DetailGrid fields={[[t('trace.detail.approvalId'), detail.data.approvalId]]} />
      ) : null}

      <JsonBlock label={t('trace.detail.attributes')} value={span.attributes} />
    </>
  )
}

function RequestShapeSummary({ value }: { value: TraceSpanDetailView }) {
  const { t } = useTranslation()
  if (value.detail.kind !== 'model_attempt') return null
  const summary = value.detail.data.requestSummary
  return (
    <DetailSection title={t('trace.detail.requestShape')}>
      <DetailGrid
        fields={[
          [t('trace.detail.requestSummaryVersion'), summary.version],
          [t('trace.detail.messageCount'), summary.messageCount],
          [t('trace.detail.messageTextChars'), summary.messageTextChars],
          [t('trace.detail.systemPromptChars'), summary.systemPromptChars],
          [t('trace.detail.toolDefinitionCount'), summary.toolDefinitionCount],
          [t('trace.detail.toolNames'), summary.toolNames.join(', ')],
          [t('trace.detail.temperature'), summary.temperature],
          [t('trace.detail.maxOutputTokens'), summary.maxOutputTokens],
          [t('trace.detail.thinkingMode'), summary.thinkingMode],
        ]}
      />
    </DetailSection>
  )
}

function DetailSection({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section>
      <h4 className="mb-2 text-[11px] font-semibold uppercase tracking-wide text-ink-faint">
        {title}
      </h4>
      {children}
    </section>
  )
}

function DetailGrid({
  fields,
}: {
  fields: Array<[string, string | number | boolean | null | undefined]>
}) {
  const visible = fields.filter(([, value]) => value !== null && value !== undefined && value !== '')
  if (visible.length === 0) return null
  return (
    <dl className="grid grid-cols-2 gap-x-4 gap-y-3 text-xs max-[620px]:grid-cols-1">
      {visible.map(([label, value]) => (
        <div key={label} className="min-w-0">
          <dt className="text-ink-faint">{label}</dt>
          <dd className="mt-1 break-words font-medium text-ink">{String(value)}</dd>
        </div>
      ))}
    </dl>
  )
}

function TimingCard({ label, value }: { label: string; value: number | null }) {
  const { i18n } = useTranslation()
  return (
    <div className="rounded-xl border border-line bg-paper-hover p-3">
      <div className="text-[11px] text-ink-faint">{label}</div>
      <div className="mt-1 text-sm font-semibold tabular-nums text-ink">
        {value == null ? '—' : formatDetailDuration(value, i18n.language)}
      </div>
    </div>
  )
}

function MetricCard({ label, value }: { label: string; value: string | number }) {
  return (
    <div className="rounded-xl border border-line bg-paper-hover p-3">
      <div className="text-[11px] text-ink-faint">{label}</div>
      <div className="mt-1 break-words text-sm font-semibold tabular-nums text-ink">
        {value}
      </div>
    </div>
  )
}

function formatDetailDuration(value: number, language: string): string {
  if (value < 1_000) return `${value} ms`
  const seconds = new Intl.NumberFormat(language, { maximumFractionDigits: 3 }).format(value / 1_000)
  return `${seconds} ${language.startsWith('zh') ? '秒' : 's'}`
}

function JsonBlock({
  label,
  value,
  splitStringContent = false,
}: {
  label: string
  value: unknown
  splitStringContent?: boolean
}) {
  const { t } = useTranslation()
  if (value === null || value === undefined) return null
  const { metadata, content } = splitStringContent
    ? extractStringContent(value)
    : { metadata: value, content: null }
  return (
    <div>
      <h4 className="mb-1 text-xs font-medium text-ink-faint">{label}</h4>
      {metadata !== null ? (
        <pre className="max-h-72 overflow-auto whitespace-pre-wrap break-words rounded-xl bg-paper-hover p-3 font-mono text-[11px] leading-relaxed text-ink-soft">
          {typeof metadata === 'string' ? metadata : JSON.stringify(metadata, null, 2)}
        </pre>
      ) : null}
      {content !== null ? (
        <div className={metadata !== null ? 'mt-2' : undefined}>
          <div className="mb-1 text-[10px] uppercase tracking-wider text-ink-faint">
            {t('tool.content')}
          </div>
          <pre
            data-tool-input-content="true"
            className="max-h-96 overflow-auto whitespace-pre-wrap break-words rounded-xl bg-paper-hover p-3 font-mono text-[11px] leading-relaxed text-ink-soft"
          >
            {content}
          </pre>
        </div>
      ) : null}
    </div>
  )
}

function extractStringContent(value: unknown): {
  metadata: unknown | null
  content: string | null
} {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return { metadata: value, content: null }
  }
  const record = value as Record<string, unknown>
  if (typeof record.content !== 'string') {
    return { metadata: value, content: null }
  }
  const { content, ...metadata } = record
  return {
    metadata: Object.keys(metadata).length > 0 ? metadata : null,
    content,
  }
}

function MessageList({
  messages,
  onRevealMessage,
}: {
  messages: SessionMessage[]
  onRevealMessage?: (messageId: string) => void
}) {
  const { t } = useTranslation()
  if (messages.length === 0) {
    return (
      <div className="rounded-xl border border-dashed border-line p-4 text-center text-xs text-ink-faint">
        {t('trace.detail.noMessages')}
      </div>
    )
  }
  return (
    <div>
      <h4 className="mb-2 text-xs font-medium text-ink-faint">{t('trace.detail.messages')}</h4>
      <div className="space-y-2">
        {messages.map((message) => (
          <button
            key={message.id}
            type="button"
            disabled={!onRevealMessage}
            onClick={() => onRevealMessage?.(message.id)}
            className="flex w-full gap-2 rounded-xl border border-line p-3 text-left disabled:cursor-default"
          >
            <MessageSquareText size={14} className="mt-0.5 shrink-0 text-ink-faint" />
            <span className="min-w-0">
              <span className="block text-[10px] uppercase text-ink-faint">{message.role}</span>
              <span className="mt-1 line-clamp-4 whitespace-pre-wrap text-xs leading-5 text-ink-soft">
                {messageText(message) || t('trace.detail.nonTextMessage')}
              </span>
            </span>
          </button>
        ))}
      </div>
    </div>
  )
}

function messageText(message: SessionMessage): string {
  return message.parts
    .filter((part) => part.type === 'text')
    .map((part) => part.text)
    .join('\n')
}

function duration(value: number | null, language: string): string | null {
  return value == null ? null : formatDuration(value, language)
}

function booleanText(
  value: boolean | null,
  t: (key: string) => string,
): string | null {
  if (value == null) return null
  return value ? t('common.yes') : t('common.no')
}

function formatTimestamp(value: number | null, language: string): string | null {
  if (value == null) return null
  return new Intl.DateTimeFormat(language, {
    dateStyle: 'medium',
    timeStyle: 'medium',
  }).format(value)
}

function spanStatusStyle(status: TraceSpanDetailView['span']['status']): string {
  if (status === 'failed' || status === 'denied') {
    return 'bg-status-danger-soft text-status-danger-ink'
  }
  if (status === 'running' || status === 'waiting') {
    return 'bg-status-warning-soft text-status-warning-ink'
  }
  if (status === 'succeeded') {
    return 'bg-status-success-soft text-status-success-ink'
  }
  return 'bg-paper-hover text-ink-faint'
}
