import { useEffect, useRef, useState, type ReactNode } from 'react'
import { CircleAlert, CircleX, Copy, Loader2, SquareTerminal } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { ToolActivityFrame } from './ToolActivityFrame'
import type { ToolActivity } from './ToolActivityList'

interface BashResultView {
  stdout: string
  stderr: string
  exitCode: number | null
  durationMs: number | null
  status: 'exited' | 'timed_out' | 'cancelled' | 'unknown'
}

interface BashToolActivityRowProps {
  activities: ToolActivity[]
  expanded?: boolean
  onExpandedChange: (expanded: boolean) => void
  onOpenTrace?: (providerToolCallId: string) => void
}

export function isBashDisplayTool(name: string): name is 'bash' {
  return name === 'bash'
}

export function bashActivityFailed(activity: ToolActivity): boolean {
  const result = parseBashOutput(activity.output)
  return activity.state === 'error'
    || activity.state === 'denied'
    || activity.state === 'interrupted'
    || result.status === 'timed_out'
    || result.status === 'cancelled'
    || result.exitCode != null && result.exitCode !== 0
}

export function BashToolActivityRow({
  activities,
  expanded: rememberedExpanded,
  onExpandedChange,
  onOpenTrace,
}: BashToolActivityRowProps) {
  const primary = activities[0]
  const results = activities.map((activity) => parseBashOutput(activity.output))
  const failed = activities.some((activity) => bashActivityFailed(activity))
  const running = activities.some((activity) => isInProgress(activity))
  const expanded = failed || (rememberedExpanded ?? true)
  const command = stringInput(primary, 'command')
  const exitCodes = results.map((result) => result.exitCode).filter((value): value is number => value != null)
  const sharedExitCode = exitCodes.length === activities.length && exitCodes.every((value) => value === exitCodes[0])
    ? exitCodes[0]
    : null
  const durations = results.map((result) => result.durationMs).filter((value): value is number => value != null)
  const durationMs = durations.length === activities.length
    ? durations.reduce((total, value) => total + value, 0)
    : null
  const terminalStatus = results.every((result) => result.status === results[0].status)
    ? results[0].status
    : 'unknown'
  const summary = (
    <BashSummary
      command={command}
      count={activities.length}
      exitCode={sharedExitCode}
      durationMs={durationMs}
      status={terminalStatus}
    />
  )

  return (
    <ToolActivityFrame
      toolCallId={primary.id}
      tier={failed ? 'failure' : 'readonly'}
      expanded={expanded}
      onExpandedChange={failed ? undefined : onExpandedChange}
      statusIcon={<BashStatusIcon activity={primary} failed={failed} running={running} />}
      summary={summary}
      onOpenTrace={onOpenTrace}
      detailsMaxHeightClass="max-h-[260px]"
    >
      {activities.map((activity, index) => (
        <BashOutputDetails
          key={activity.id}
          activity={activity}
          result={results[index]}
          grouped={activities.length > 1}
          onOpenTrace={onOpenTrace}
        />
      ))}
    </ToolActivityFrame>
  )
}

function BashSummary({
  command,
  count,
  exitCode,
  durationMs,
  status,
}: {
  command: string
  count: number
  exitCode: number | null
  durationMs: number | null
  status: BashResultView['status']
}) {
  const { t } = useTranslation()
  return (
    <>
      <span className="shrink-0 text-xs font-medium text-ink-soft" title="bash">
        {t('tool.bash.action')}
      </span>
      {command ? <Separator /> : null}
      {command ? <MiddleEllipsisCommand command={command} /> : null}
      {count > 1 ? <Separator /> : null}
      {count > 1 ? <span className="shrink-0 text-xs text-ink-faint">×{count}</span> : null}
      {exitCode != null ? <Separator /> : null}
      {exitCode != null ? (
        <span className="shrink-0 text-xs text-ink-faint">
          {t('tool.bash.exitCode', { code: exitCode })}
        </span>
      ) : null}
      {status === 'timed_out' || status === 'cancelled' ? <Separator /> : null}
      {status === 'timed_out' || status === 'cancelled' ? (
        <span className="shrink-0 text-xs text-status-danger-ink">
          {t(status === 'timed_out' ? 'tool.bash.timedOut' : 'tool.bash.cancelled')}
        </span>
      ) : null}
      {durationMs != null && durationMs > 300 ? <Separator /> : null}
      {durationMs != null && durationMs > 300 ? (
        <span className="shrink-0 text-xs text-ink-faint">{formatDuration(durationMs)}</span>
      ) : null}
    </>
  )
}

function MiddleEllipsisCommand({ command }: { command: string }) {
  if (command.length <= 88) {
    return (
      <span className="min-w-0 truncate font-mono text-xs text-ink" title={command}>
        {command}
      </span>
    )
  }
  return (
    <span className="flex min-w-0 items-center font-mono text-xs text-ink" title={command}>
      <span data-command-prefix="true" className="min-w-0 truncate">{command.slice(0, 56)}</span>
      <span className="shrink-0 text-ink-faint">…</span>
      <span data-command-suffix="true" className="max-w-[40%] shrink-0 truncate">{command.slice(-32)}</span>
    </span>
  )
}

function BashOutputDetails({
  activity,
  result,
  grouped,
  onOpenTrace,
}: {
  activity: ToolActivity
  result: BashResultView
  grouped: boolean
  onOpenTrace?: (providerToolCallId: string) => void
}) {
  const { t } = useTranslation()
  const failed = bashActivityFailed(activity)
  const stdoutLines = countLines(result.stdout)
  const stderrLines = countLines(result.stderr)
  const command = stringInput(activity, 'command')

  async function copyOutput() {
    const text = [result.stdout, result.stderr].filter(Boolean).join('\n')
    try {
      await navigator.clipboard?.writeText(text)
    } catch {
      // 剪贴板不可用时保留可选择的原始输出。
    }
  }

  return (
    <section data-bash-output={activity.id} className="bg-paper/70">
      {grouped ? (
        <div className="truncate border-b border-line px-3 py-1.5 font-mono text-xs text-ink-soft" title={command}>
          {command}
        </div>
      ) : null}
      <div className="sticky top-0 z-10 flex min-h-8 items-center gap-2 border-b border-line bg-paper px-3 text-[11px] text-ink-faint">
        <span>{t('tool.bash.stdoutLines', { count: stdoutLines })}</span>
        {stderrLines > 0 ? <span>{t('tool.bash.stderrLines', { count: stderrLines })}</span> : null}
        {result.exitCode != null ? <span>{t('tool.bash.exitCode', { code: result.exitCode })}</span> : null}
        {result.status === 'timed_out' ? <span>{t('tool.bash.timedOut')}</span> : null}
        {result.status === 'cancelled' ? <span>{t('tool.bash.cancelled')}</span> : null}
        {result.durationMs != null && result.durationMs > 300 ? <span>{formatDuration(result.durationMs)}</span> : null}
        <button
          type="button"
          aria-label={t('tool.copy')}
          title={t('tool.copy')}
          onClick={() => void copyOutput()}
          className="ml-auto grid size-6 place-items-center rounded-md hover:bg-paper-hover hover:text-ink"
        >
          <Copy size={12} />
        </button>
      </div>
      <BoundedBashOutput stdout={result.stdout} stderr={result.stderr} failed={failed} />
      {!result.stdout && !result.stderr ? (
        <div className="px-3 py-2 text-xs text-ink-faint">{t('tool.readonly.noOutput')}</div>
      ) : null}
      {failed ? (
        <div className="flex items-center gap-2 border-t border-status-danger-border bg-paper px-3 py-2">
          <button
            type="button"
            onClick={() => void copyOutput()}
            className="inline-flex items-center gap-1.5 rounded-md border border-status-danger-border px-2.5 py-1 text-xs text-status-danger-ink hover:bg-status-danger-soft"
          >
            <Copy size={11} />
            {t('tool.bash.copyError')}
          </button>
          {onOpenTrace ? (
            <button
              type="button"
              onClick={() => onOpenTrace(activity.id)}
              className="rounded-md border border-status-danger-border px-2.5 py-1 text-xs text-status-danger-ink hover:bg-status-danger-soft"
            >
              {t('tool.bash.inspectTrace')}
            </button>
          ) : null}
        </div>
      ) : null}
    </section>
  )
}

function BoundedBashOutput({
  stdout,
  stderr,
  failed,
}: {
  stdout: string
  stderr: string
  failed: boolean
}) {
  const { t } = useTranslation()
  const [showAll, setShowAll] = useState(false)
  const lines = [
    ...splitOutputLines(stdout).map((content) => ({ stream: 'stdout' as const, content })),
    ...splitOutputLines(stderr).map((content) => ({ stream: 'stderr' as const, content })),
  ]
  const bounded = lines.length > 2_000 && !showAll
  const hiddenCount = Math.max(0, lines.length - 400)
  const visible = bounded
    ? [
        ...lines.slice(0, 200),
        { stream: 'marker' as const, content: '' },
        ...lines.slice(-200),
      ]
    : lines
  const firstErrorIndex = failed ? visible.findIndex((line) => line.stream === 'stderr') : -1
  const firstErrorRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (failed) firstErrorRef.current?.scrollIntoView({ block: 'nearest' })
  }, [failed])

  return (
    <div className="bg-code-bg font-mono text-xs leading-5">
      {visible.map((line, index) => (
        line.stream === 'marker' ? (
          <div key="hidden-lines" className="border-y border-line bg-paper px-3 py-1 text-xs text-ink-faint">
            {t('tool.bash.hiddenLines', { count: hiddenCount })}
          </div>
        ) : (
          <div
            key={`${bounded && index > 200 ? index + hiddenCount : index}-${line.stream}`}
            ref={index === firstErrorIndex ? firstErrorRef : undefined}
            data-bash-output-line={line.stream}
            data-first-error-line={index === firstErrorIndex ? 'true' : undefined}
            className={`whitespace-pre-wrap break-words px-3 ${
              line.stream === 'stderr' && failed
                ? 'bg-status-danger-soft text-status-danger-ink'
                : line.stream === 'stderr'
                  ? 'bg-status-warning-soft text-status-warning-ink'
                : 'text-ink-soft'
            }`}
          >
            {line.content || '\u00a0'}
          </div>
        )
      ))}
      {lines.length > 2_000 ? (
        <button
          type="button"
          aria-expanded={showAll}
          onClick={() => setShowAll((value) => !value)}
          className="sticky bottom-0 w-full border-t border-line bg-paper px-3 py-1.5 text-left text-xs text-clay hover:bg-paper-hover"
        >
          {showAll ? t('tool.bash.collapseOutput') : t('tool.readonly.loadFullOutput')}
        </button>
      ) : null}
    </div>
  )
}

function BashStatusIcon({
  activity,
  failed,
  running,
}: {
  activity: ToolActivity
  failed: boolean
  running: boolean
}) {
  const { t } = useTranslation()
  if (running) return <Loader2 size={14} aria-label={t('tool.running')} className="shrink-0 animate-spin text-ink-faint" />
  if (failed) return <CircleAlert size={14} aria-label={t('tool.error')} className="shrink-0 text-status-danger" />
  if (activity.state === 'denied' || activity.state === 'interrupted') {
    return <CircleX size={14} aria-label={t('tool.stopped')} className="shrink-0 text-status-danger" />
  }
  // 成功态图标是装饰：动作名就在紧邻的摘要里，读屏再念一遍只是噪音。
  return <SquareTerminal size={14} aria-hidden="true" className="shrink-0 text-ink-faint" />
}

function parseBashOutput(output: string): BashResultView {
  const exited = output.match(/(?:^|\n)\[exit (-?\d+); duration (\d+) ms\]\s*$/)
  const timedOut = output.match(/(?:^|\n)\[timed out after \d+ ms; duration (\d+) ms\]\s*$/)
  const cancelled = output.match(/(?:^|\n)\[cancelled; duration (\d+) ms\]\s*$/)
  const footer = exited?.[0] ?? timedOut?.[0] ?? cancelled?.[0] ?? ''
  const body = footer ? output.slice(0, -footer.length) : output
  const stderrPrefix = '[stderr]\n'
  const stderrMarker = '\n[stderr]\n'
  const markerIndex = body.indexOf(stderrMarker)
  const stderrOnly = body.startsWith(stderrPrefix)
  const stdout = (stderrOnly ? '' : markerIndex >= 0 ? body.slice(0, markerIndex) : body).replace(/^\n|\n$/g, '')
  const stderr = (
    stderrOnly
      ? body.slice(stderrPrefix.length)
      : markerIndex >= 0
        ? body.slice(markerIndex + stderrMarker.length)
        : ''
  ).replace(/^\n|\n$/g, '')
  return {
    stdout,
    stderr,
    exitCode: exited ? Number(exited[1]) : null,
    durationMs: exited
      ? Number(exited[2])
      : timedOut
        ? Number(timedOut[1])
        : cancelled
          ? Number(cancelled[1])
          : null,
    status: exited ? 'exited' : timedOut ? 'timed_out' : cancelled ? 'cancelled' : 'unknown',
  }
}

function countLines(value: string): number {
  return value ? value.split('\n').length : 0
}

function splitOutputLines(value: string): string[] {
  return value ? value.split('\n') : []
}

function formatDuration(durationMs: number): string {
  if (durationMs < 1_000) return `${durationMs}ms`
  const seconds = durationMs / 1_000
  return `${Number.isInteger(seconds) ? seconds.toFixed(0) : seconds.toFixed(1)}s`
}

function stringInput(activity: ToolActivity, key: string): string {
  const value = activity.input?.[key]
  return typeof value === 'string' ? value : ''
}

function isInProgress(activity: ToolActivity): boolean {
  return activity.state === 'pending' || activity.state === 'submitted' || activity.state === 'running'
}

function Separator(): ReactNode {
  return <span aria-hidden="true" className="shrink-0 text-[10px] text-ink-faint/70">·</span>
}
