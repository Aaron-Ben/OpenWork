import { useEffect, useMemo, useRef, useState } from 'react'
import type { TFunction } from 'i18next'
import { useTranslation } from 'react-i18next'
import {
  Check,
  ChevronDown,
  CircleAlert,
  FileText,
  Pencil,
  ShieldAlert,
  Terminal,
  X,
} from 'lucide-react'

import type {
  RuntimeEffectDisplay,
  RuntimeApprovalSessionAction,
  RuntimePermissionCardUnit,
  RuntimePermissionDecision,
  RuntimePermissionRequest,
  RuntimeUnitVerdict,
} from '@/bridge/compat'
import { useRuntimeStore } from '../runtimeStore'
import { useTurnActions } from '../useTurn'
import { FileDiffContent, type FileDiffHunk } from './FileDiffPanel'

interface ApprovalCardViewProps {
  request: RuntimePermissionRequest
  resolving: boolean
  onResolve: (decision: RuntimePermissionDecision) => void
  workspaceRoot?: string
  toolInput?: unknown
}

interface ApprovalPreview {
  kind: 'edit' | 'write'
  hunk: FileDiffHunk
}

function relativePath(path: string, workspaceRoot?: string): string {
  if (!workspaceRoot) return path
  const normalizedRoot = workspaceRoot.replace(/\/$/, '')
  if (path === normalizedRoot) return '.'
  return path.startsWith(`${normalizedRoot}/`) ? path.slice(normalizedRoot.length + 1) : path
}

function relativeDisplay(display: string, workspaceRoot?: string): string {
  if (!workspaceRoot) return display
  const normalizedRoot = workspaceRoot.replace(/\/$/, '')
  return display
    .split(`${normalizedRoot}/`).join('')
    .split(normalizedRoot).join('.')
    .replace(/^\s+/, '')
}

function effectLabel(t: TFunction, display: RuntimeEffectDisplay, workspaceRoot?: string): string {
  if (display.certainty === 'trusted_program') {
    return t('tool.permission.trustedProgram', { program: display.program })
  }
  if (display.certainty === 'readonly_proof') {
    return t('tool.permission.readonlyProof')
  }

  switch (display.effect.kind) {
    case 'read':
      return t('tool.permission.read', { path: relativePath(display.effect.path, workspaceRoot) })
    case 'write':
      return t('tool.permission.write', { path: relativePath(display.effect.path, workspaceRoot) })
    case 'exec':
      return t('tool.permission.exec', {
        command: [display.effect.program, ...display.effect.args].join(' '),
      })
  }
}

function verdictLabel(t: TFunction, verdict: RuntimeUnitVerdict): string {
  if (verdict.decision === 'deny') return t('tool.permission.deniedByRule')
  if (verdict.decision === 'allow') return t('tool.permission.autoAllowed')
  switch (verdict.source) {
    case 'builtin_sensitive':
      return t('tool.permission.sensitivePath')
    case 'unparsed':
      return t('tool.permission.unparsed')
    case 'no_rule_covers':
      return t('tool.permission.noRuleCovers')
  }
}

function sessionActionDescription(t: TFunction, action: RuntimeApprovalSessionAction): string {
  if (action.kind === 'enable_accept_edits') {
    return t('tool.permission.enableAcceptEditsDescription')
  }
  if (action.grants.length === 1 && action.grants[0].exact) {
    return t('tool.permission.allowExactForSession', { command: action.grants[0].label })
  }
  if (action.grants.length === 1) {
    return t('tool.permission.allowPrefixForSession', { command: action.grants[0].label })
  }
  return t('tool.permission.allowManyForSession', {
    commands: action.grants.map((grant) => grant.label).join(', '),
  })
}

function unitWrites(unit: RuntimePermissionCardUnit): boolean {
  return unit.effects.some(
    (display) => display.certainty === 'inferred' && display.effect.kind === 'write',
  )
}

function writePaths(units: RuntimePermissionCardUnit[]): string[] {
  return [...new Set(units.flatMap((unit) => unit.effects.flatMap((display) => (
    display.certainty === 'inferred' && display.effect.kind === 'write'
      ? [display.effect.path]
      : []
  ))))]
}

function unitWritePath(unit: RuntimePermissionCardUnit): string | null {
  for (const display of unit.effects) {
    if (display.certainty === 'inferred' && display.effect.kind === 'write') {
      return display.effect.path
    }
  }
  return null
}

function commandSummary(display: string): string {
  const tokens = display.trim().split(/\s+/)
  if (tokens[0] === 'node' && tokens[1]?.startsWith('--')) return tokens.slice(0, 2).join(' ')
  return tokens[0] ?? display
}

function isPermanentDelete(raw: string): boolean {
  return /(?:^|[;&|]\s*)rm\s+(?:-[^\s]*r[^\s]*f|-[^\s]*f[^\s]*r|--recursive\s+--force|--force\s+--recursive)(?:\s|$)/i.test(raw)
}

function isForcePush(raw: string): boolean {
  return /\bgit\s+push\b[^\n;&|]*(?:--force(?:-with-lease)?|-f)(?:\s|$)/i.test(raw)
}

function asRecord(input: unknown): Record<string, unknown> | null {
  return input != null && typeof input === 'object' && !Array.isArray(input)
    ? input as Record<string, unknown>
    : null
}

function textField(input: Record<string, unknown>, camel: string, snake: string): string | null {
  const value = input[camel] ?? input[snake]
  return typeof value === 'string' ? value : null
}

/** 审批阶段没有文件快照；这里只展示工具入参能严格证明的替换片段或拟写入内容。 */
function approvalPreview(toolName: string, toolInput: unknown): ApprovalPreview | null {
  const input = asRecord(toolInput)
  if (!input) return null
  if (toolName === 'edit') {
    const oldText = textField(input, 'oldString', 'old_string')
    const newText = textField(input, 'newString', 'new_string')
    if (oldText == null || newText == null) return null
    const deletedLines = oldText.split('\n').map((content, index) => ({
      kind: 'deletion' as const,
      oldLine: index + 1,
      newLine: null,
      content,
    }))
    const addedLines = newText.split('\n').map((content, index) => ({
      kind: 'addition' as const,
      oldLine: null,
      newLine: index + 1,
      content,
    }))
    return {
      kind: 'edit',
      hunk: {
        oldStart: 1,
        oldLines: deletedLines.length,
        newStart: 1,
        newLines: addedLines.length,
        lines: [...deletedLines, ...addedLines],
      },
    }
  }
  if (toolName === 'write') {
    const content = textField(input, 'content', 'content')
    if (content == null) return null
    const addedLines = content.split('\n').map((line, index) => ({
      kind: 'addition' as const,
      oldLine: null,
      newLine: index + 1,
      content: line,
    }))
    return {
      kind: 'write',
      hunk: {
        oldStart: 0,
        oldLines: 0,
        newStart: 1,
        newLines: addedLines.length,
        lines: addedLines,
      },
    }
  }
  return null
}

function EffectRow({ display, workspaceRoot }: { display: RuntimeEffectDisplay; workspaceRoot?: string }) {
  const { t } = useTranslation()
  const Icon = display.certainty === 'trusted_program'
    ? Terminal
    : display.certainty === 'readonly_proof'
      ? FileText
      : display.effect.kind === 'write'
        ? Pencil
        : FileText
  const fullPath = display.certainty === 'inferred' && display.effect.kind !== 'exec'
    ? display.effect.path
    : undefined
  return (
    <li className="flex min-w-0 items-start gap-2 text-xs text-ink-soft" title={fullPath}>
      <Icon className="mt-0.5 size-3.5 shrink-0 text-ink-faint" />
      <span className="min-w-0 truncate font-mono">{effectLabel(t, display, workspaceRoot)}</span>
    </li>
  )
}

function PreviewDiff({ preview }: { preview: ApprovalPreview }) {
  const { t } = useTranslation()
  return (
    <div className="border-t border-line bg-paper/70">
      <div
        className="bg-paper-hover px-3 py-1.5 font-mono text-[11px] text-ink-faint"
        title={t('tool.permission.previewLimitation')}
      >
        @@ {preview.kind === 'edit'
          ? t('tool.permission.replacementPreview')
          : t('tool.permission.writePreview')}
      </div>
      <div className="max-h-[240px] overflow-auto overscroll-contain">
        <FileDiffContent
          change={{ changeId: `approval-${preview.kind}`, hunks: [preview.hunk] }}
          showHunkHeaders={false}
        />
      </div>
    </div>
  )
}

function WriteUnitCard({
  unit,
  preview,
  workspaceRoot,
}: {
  unit: RuntimePermissionCardUnit
  preview: ApprovalPreview | null
  workspaceRoot?: string
}) {
  const { t } = useTranslation()
  const path = unitWritePath(unit)
  const additions = preview?.hunk.lines.filter((line) => line.kind === 'addition').length ?? null
  const deletions = preview?.hunk.lines.filter((line) => line.kind === 'deletion').length ?? null

  return (
    <li
      data-permission-unit="true"
      data-approval-write-summary="true"
      className={`overflow-hidden rounded-lg border ${
        unit.outsideWorkspace
          ? 'border-status-danger-border bg-status-danger-soft/35'
          : 'border-line bg-paper'
      }`}
    >
      <div className="flex min-h-9 min-w-0 items-center gap-2 px-3 py-2 text-xs">
        <span className="shrink-0 font-semibold text-ink">{t('tool.permission.writeLabel')}</span>
        {path ? (
          <code className="min-w-0 truncate font-semibold text-ink" title={path}>
            {relativePath(path, workspaceRoot)}
          </code>
        ) : null}
        <span className="shrink-0 text-ink-faint">
          {unit.outsideWorkspace
            ? t('tool.permission.outsideWrite')
            : t('tool.permission.insideWorkspace')}
        </span>
        {additions != null ? (
          <span className="ml-auto shrink-0 font-mono text-status-success-ink">+{additions}</span>
        ) : null}
        {deletions != null && deletions > 0 ? (
          <span className="shrink-0 font-mono text-status-danger-ink">−{deletions}</span>
        ) : null}
      </div>
      {preview ? <PreviewDiff preview={preview} /> : null}
    </li>
  )
}

export function ApprovalCardView({
  request,
  resolving,
  onResolve,
  workspaceRoot,
  toolInput,
}: ApprovalCardViewProps) {
  const { t } = useTranslation()
  const titleRef = useRef<HTMLDivElement>(null)
  const cancelRef = useRef<HTMLButtonElement>(null)
  const preview = useMemo(
    () => approvalPreview(request.toolName, toolInput),
    [request.toolName, toolInput],
  )
  const decisionUnits = request.card.units.filter((unit) => unit.verdict.decision !== 'allow')
  const readonlyUnits = request.card.units.filter(
    (unit) => unit.verdict.decision === 'allow' && !unitWrites(unit) && !unit.outsideWorkspace,
  )
  // 已放行的写入仍会改变用户数据，不能跟只读项一起折叠掉。
  const prominentUnits = request.card.units.filter((unit) => !readonlyUnits.includes(unit))
  const writes = writePaths(prominentUnits)
  const previewUnitIndex = prominentUnits.findIndex((unit) => unitWrites(unit))
  const outsideWrite = prominentUnits.some((unit) => unit.outsideWorkspace && unitWrites(unit))
  const permanentDelete = isPermanentDelete(request.card.raw)
  const forcePush = isForcePush(request.card.raw)
  const dangerous = outsideWrite || permanentDelete || forcePush
  const firstDecisionIndex = request.card.units.findIndex((unit) => unit.verdict.decision !== 'allow')

  const title = permanentDelete
    ? t('tool.permission.deleteTitle')
    : forcePush
      ? t('tool.permission.forcePushTitle')
      : outsideWrite
        ? t('tool.permission.outsideTitle')
        : writes.length > 0 && readonlyUnits.length > 0
          ? t('tool.permission.writeMixedTitle', { writes: writes.length, readonly: readonlyUnits.length })
          : writes.length > 0
            ? t('tool.permission.writeTitle', { count: writes.length })
            : t('tool.permission.approvalTitle', { count: Math.max(1, decisionUnits.length) })
  const subtitle = permanentDelete
    ? t('tool.permission.deleteSubtitle')
    : forcePush
      ? t('tool.permission.forcePushSubtitle')
      : outsideWrite
        ? t('tool.permission.outsideSubtitle')
        : decisionUnits.length === 1 && request.card.units.length > 1
          ? t('tool.permission.singleDecisionSubtitle', { index: firstDecisionIndex + 1 })
          : t('tool.permission.askSubtitle')

  useEffect(() => {
    if (dangerous) cancelRef.current?.focus()
    else titleRef.current?.focus()
  }, [dangerous, request.toolCallId])

  function handleKeyDown(event: React.KeyboardEvent<HTMLDivElement>) {
    if (resolving) return
    const target = event.target as HTMLElement
    if (event.key === 'Escape') {
      event.preventDefault()
      onResolve('deny')
      return
    }
    if (event.key !== 'Enter' || target.closest('button, summary')) return
    event.preventDefault()
    onResolve(dangerous ? 'deny' : 'allow_once')
  }

  return (
    <div
      data-approval-dialog="true"
      data-approval-danger={dangerous ? 'true' : undefined}
      role="alertdialog"
      aria-modal="false"
      aria-labelledby="permission-title"
      className={`overflow-hidden rounded-xl bg-paper shadow-sm ${
        dangerous ? 'border-2 border-status-danger-border' : 'border border-clay-soft'
      }`}
      onKeyDown={handleKeyDown}
    >
      <div className={`flex items-center gap-3 px-4 py-3 ${
        dangerous ? 'bg-status-danger-soft' : 'bg-clay-soft'
      }`}>
        <div className="grid size-8 place-items-center rounded-lg bg-paper shadow-sm ring-1 ring-clay-soft">
          {dangerous
            ? <CircleAlert className="text-status-danger-ink" size={18} />
            : <ShieldAlert className="text-clay" size={18} />}
        </div>
        <div ref={titleRef} id="permission-title" tabIndex={-1} className="min-w-0 flex-1 outline-none">
          <div className="text-sm font-semibold text-ink">{title}</div>
          <div className={`mt-0.5 text-xs ${dangerous ? 'text-status-danger-ink' : 'text-ink-muted'}`}>
            {subtitle}
          </div>
        </div>
      </div>

      <div data-approval-scroll="true" className="max-h-[45vh] overflow-y-auto overscroll-contain">
        {readonlyUnits.length > 0 ? (
          <details className="group border-b border-line px-4 py-2.5">
            <summary
              className="flex cursor-pointer list-none items-center gap-2 text-xs text-ink-muted"
              title={readonlyUnits.flatMap((unit) => unit.verdict.ruleId ?? []).join(', ') || undefined}
            >
              <Check size={13} className="shrink-0 text-status-success-ink" aria-hidden="true" />
              <span className="min-w-0 flex-1 truncate">
                {t('tool.permission.readonlySummary', {
                  count: readonlyUnits.length,
                  commands: readonlyUnits.map((unit) => commandSummary(unit.display)).join(' · '),
                })}
              </span>
              <ChevronDown size={13} className="shrink-0 transition-transform group-open:rotate-180" />
            </summary>
            <ol className="mt-2 space-y-1 pl-5">
              {readonlyUnits.map((unit, index) => (
                <li key={`${index}-${unit.display}`} className="truncate font-mono text-[11px] text-ink-faint">
                  {relativeDisplay(unit.display, workspaceRoot)}
                </li>
              ))}
            </ol>
          </details>
        ) : null}

        <ol className="space-y-2 px-4 py-3">
          {prominentUnits.map((unit, index) => {
            const originalIndex = request.card.units.indexOf(unit)
            const write = unitWrites(unit)
            const unitPreview = index === previewUnitIndex ? preview : null
            if (write && (request.toolName === 'edit' || request.toolName === 'write')) {
              return (
                <WriteUnitCard
                  key={`${originalIndex}-${unit.display}`}
                  unit={unit}
                  preview={unitPreview}
                  workspaceRoot={workspaceRoot}
                />
              )
            }
            return (
              <li
                key={`${originalIndex}-${unit.display}`}
                data-permission-unit="true"
                className={`rounded-lg border px-3 py-2.5 ${
                  unit.outsideWorkspace && write
                    ? 'border-status-danger-border bg-status-danger-soft/55'
                    : unit.outsideWorkspace
                      ? 'border-status-warning-border bg-status-warning-soft/55'
                    : write
                      ? 'border-status-success-border bg-status-success-soft/45'
                      : 'border-line bg-paper-hover'
                }`}
              >
                <div className="flex items-start gap-2">
                  <span className="shrink-0 text-xs font-semibold text-ink-faint">{originalIndex + 1}.</span>
                  <code
                    className="max-h-40 min-w-0 flex-1 overflow-y-auto overscroll-contain whitespace-pre-wrap break-words text-xs font-semibold text-ink"
                    title={unit.display}
                  >
                    {relativeDisplay(unit.display, workspaceRoot)}
                  </code>
                  {unit.outsideWorkspace ? (
                    <span className="shrink-0 rounded-full bg-status-danger-soft px-2 py-0.5 text-[10px] font-medium text-status-danger-ink">
                      {t(write ? 'tool.permission.outsideWrite' : 'tool.permission.outsideWorkspace')}
                    </span>
                  ) : write ? (
                    <span className="shrink-0 rounded-full bg-status-success-soft px-2 py-0.5 text-[10px] font-medium text-status-success-ink">
                      {t('tool.permission.writeLabel')}
                    </span>
                  ) : null}
                </div>
                <ul className="mt-2 space-y-1 pl-5">
                  {unit.effects.map((effect, effectIndex) => (
                    <EffectRow key={effectIndex} display={effect} workspaceRoot={workspaceRoot} />
                  ))}
                  {unit.effects.length === 0 ? (
                    <li className="text-xs text-status-warning-ink">{t('tool.permission.unknownEffects')}</li>
                  ) : null}
                </ul>
                <div
                  className="mt-2 pl-5 text-[11px] text-ink-faint"
                  title={unit.verdict.ruleId ?? undefined}
                >
                  {verdictLabel(t, unit.verdict)}
                </div>
              </li>
            )
          })}
        </ol>

        {request.card.unparsed ? (
          <div className="mx-4 mb-3 rounded-lg bg-status-warning-soft px-3 py-2 text-xs text-status-warning-ink">
            {t('tool.permission.unparsedWarning')}
          </div>
        ) : null}

        <details className="group border-t border-line px-4 py-3">
          <summary className="inline-flex cursor-pointer list-none items-center gap-1 text-xs text-ink-muted hover:text-ink">
            {t('tool.permission.showRaw')}
            <ChevronDown size={13} className="transition-transform group-open:rotate-180" />
          </summary>
          <pre className="mt-2 max-h-32 overflow-auto overscroll-contain whitespace-pre-wrap break-words rounded-lg bg-paper-hover px-3 py-2.5 font-mono text-[11px] leading-relaxed text-ink-soft">
            {request.card.raw}
          </pre>
        </details>
      </div>

      <div className="border-t border-clay-soft bg-paper-hover px-4 py-3">
        {dangerous ? (
          <div className="flex flex-nowrap items-center gap-2 overflow-x-auto">
            <button
              ref={cancelRef}
              type="button"
              disabled={resolving}
              onClick={() => onResolve('deny')}
              className="inline-flex min-h-8 shrink-0 items-center gap-1.5 whitespace-nowrap rounded-lg bg-clay px-4 py-1.5 text-sm font-medium text-paper transition hover:opacity-90 disabled:opacity-50"
            >
              <X size={14} />
              {t('tool.permission.cancel')}
            </button>
            <button
              type="button"
              disabled={resolving}
              onClick={() => onResolve('allow_once')}
              className="inline-flex min-h-8 shrink-0 items-center gap-1.5 whitespace-nowrap rounded-lg border border-status-danger-border bg-paper px-4 py-1.5 text-sm font-medium text-status-danger-ink transition hover:bg-status-danger-soft disabled:opacity-50"
            >
              {resolving
                ? t('tool.processing')
                : permanentDelete
                  ? t('tool.permission.deleteAnyway')
                  : t('tool.permission.allowDanger')}
            </button>
          </div>
        ) : (
          <>
            <div className="flex flex-nowrap items-center gap-2 overflow-x-auto">
              <button
                type="button"
                disabled={resolving}
                onClick={() => onResolve('allow_once')}
                className="inline-flex min-h-8 shrink-0 items-center gap-1.5 whitespace-nowrap rounded-lg bg-clay px-4 py-1.5 text-sm font-medium text-paper transition hover:opacity-90 disabled:opacity-50"
              >
                <Check size={14} />
                {resolving ? t('tool.processing') : t('tool.permission.allowOnce')}
              </button>
              {request.card.sessionAction ? (
                <button
                  type="button"
                  disabled={resolving}
                  onClick={() => onResolve(
                    request.card.sessionAction?.kind === 'allow_exec'
                      ? 'allow_session'
                      : 'accept_edits',
                  )}
                  className="inline-flex min-h-8 shrink-0 items-center whitespace-nowrap rounded-lg border border-line-strong bg-paper px-4 py-1.5 text-sm font-medium text-ink transition hover:bg-paper-hover disabled:opacity-50"
                >
                  {t('tool.permission.alwaysAllowSession')}
                </button>
              ) : null}
              <button
                type="button"
                disabled={resolving}
                onClick={() => onResolve('deny')}
                className="ml-auto inline-flex min-h-8 shrink-0 items-center gap-1.5 whitespace-nowrap rounded-lg px-4 py-1.5 text-sm font-medium text-ink-muted transition hover:bg-paper disabled:opacity-50"
              >
                <X size={14} />
                {t('tool.reject')}
              </button>
            </div>
            {request.card.sessionAction ? (
              <p className="mt-1.5 text-[11px] text-ink-faint">
                {sessionActionDescription(t, request.card.sessionAction)}
              </p>
            ) : null}
          </>
        )}
        <p className="mt-1.5 text-[11px] text-ink-faint">
          {dangerous ? t('tool.permission.dangerShortcuts') : t('tool.permission.shortcuts')}
        </p>
      </div>
    </div>
  )
}

/// 输入框上方的权限卡片。Permission 属于对应 Session Runtime，直到 Core 事件确认已处理。
export function ApprovalDialog({
  sessionId,
  workspaceRoot,
}: {
  sessionId: string | null
  workspaceRoot?: string
}) {
  const runtime = useRuntimeStore((state) => sessionId ? state.bySession[sessionId] : undefined)
  const current = runtime?.pendingPermission ?? null
  const toolInput = current ? runtime?.toolCalls[current.toolCallId]?.input : undefined
  const { resolvePermission } = useTurnActions(sessionId)
  const [resolving, setResolving] = useState(false)

  if (!current) return null

  return (
    <ApprovalCardView
      request={current}
      resolving={resolving}
      workspaceRoot={workspaceRoot}
      toolInput={toolInput}
      onResolve={(decision) => {
        if (resolving) return
        setResolving(true)
        void resolvePermission(decision).finally(() => setResolving(false))
      }}
    />
  )
}
