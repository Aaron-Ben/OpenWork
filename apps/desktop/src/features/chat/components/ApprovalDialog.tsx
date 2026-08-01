import { useEffect, useRef, useState } from 'react'
import type { TFunction } from 'i18next'
import { useTranslation } from 'react-i18next'
import { Check, FileText, Pencil, ShieldAlert, Terminal, X } from 'lucide-react'

import type {
  RuntimeEffectDisplay,
  RuntimeApprovalSessionAction,
  RuntimePermissionDecision,
  RuntimePermissionRequest,
  RuntimeUnitVerdict,
} from '../../../bridge/compat'
import { useRuntimeStore } from '../runtimeStore'
import { useTurnActions } from '../useTurn'

function effectLabel(t: TFunction, display: RuntimeEffectDisplay): string {
  if (display.certainty === 'trusted_program') {
    return t('tool.permission.trustedProgram', { program: display.program })
  }
  if (display.certainty === 'readonly_proof') {
    return t('tool.permission.readonlyProof', { key: display.key })
  }

  switch (display.effect.kind) {
    case 'read':
      return t('tool.permission.read', { path: display.effect.path })
    case 'write':
      return t('tool.permission.write', { path: display.effect.path })
    case 'exec':
      return t('tool.permission.exec', {
        command: [display.effect.program, ...display.effect.args].join(' '),
      })
  }
}

function verdictLabel(t: TFunction, verdict: RuntimeUnitVerdict): string {
  if (verdict.decision === 'deny') return t('tool.permission.deniedByRule')
  if (verdict.decision === 'allow') {
    switch (verdict.source) {
      case 'mode':
        return t('tool.permission.allowedByMode')
      case 'readonly_proof':
        return t('tool.permission.allowedByReadonlyProof')
      case 'session_grant':
        return t('tool.permission.allowedBySessionGrant')
      case 'rule':
        return t('tool.permission.allowedByRule')
      case 'builtin':
        return t('tool.permission.allowedByBuiltin')
    }
  }
  switch (verdict.source) {
    case 'explicit_rule':
      return t('tool.permission.explicitAsk')
    case 'builtin_sensitive':
      return t('tool.permission.sensitivePath')
    case 'unparsed':
      return t('tool.permission.unparsed')
    case 'no_rule_covers':
      return t('tool.permission.noRuleCovers')
  }
}

function sessionActionLabel(t: TFunction, action: RuntimeApprovalSessionAction): string {
  if (action.kind === 'enable_accept_edits') {
    return t('tool.permission.enableAcceptEdits')
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

function EffectRow({ display }: { display: RuntimeEffectDisplay }) {
  const { t } = useTranslation()
  const Icon = display.certainty === 'trusted_program'
    ? Terminal
    : display.certainty === 'readonly_proof'
      ? FileText
      : display.effect.kind === 'write'
        ? Pencil
        : FileText
  return (
    <li className="flex min-w-0 items-start gap-2 text-xs text-ink-soft">
      <Icon className="mt-0.5 size-3.5 shrink-0 text-ink-faint" />
      <span className="break-all font-mono">{effectLabel(t, display)}</span>
    </li>
  )
}

export function ApprovalCardView({
  request,
  resolving,
  onResolve,
}: {
  request: RuntimePermissionRequest
  resolving: boolean
  onResolve: (decision: RuntimePermissionDecision) => void
}) {
  const { t } = useTranslation()
  const titleRef = useRef<HTMLDivElement>(null)

  useEffect(() => titleRef.current?.focus(), [request.toolCallId])

  return (
    <div
      data-approval-dialog="true"
      role="alertdialog"
      aria-modal="false"
      aria-labelledby="permission-title"
      className="overflow-hidden rounded-xl border border-clay-soft bg-paper shadow-sm"
      onKeyDown={(event) => {
        if (event.key === 'Escape') {
          event.preventDefault()
          onResolve('deny')
        }
      }}
    >
      <div className="flex items-center gap-3 bg-clay-soft px-4 py-3">
        <div className="grid size-8 place-items-center rounded-lg bg-paper shadow-sm ring-1 ring-clay-soft">
          <ShieldAlert className="text-clay" size={18} />
        </div>
        <div ref={titleRef} id="permission-title" tabIndex={-1} className="min-w-0 flex-1 outline-none">
          <div className="text-sm font-semibold text-ink">
            {t('tool.permission.title', { count: request.card.units.length })}
          </div>
          <div className="mt-0.5 text-xs text-clay">{t('tool.waitingApproval')}</div>
        </div>
      </div>

      <ol className="space-y-2 border-t border-clay-soft px-4 py-3">
        {request.card.units.map((unit, index) => (
          <li
            key={`${index}-${unit.display}`}
            data-permission-unit="true"
            className="rounded-lg bg-paper-hover px-3 py-2.5"
          >
            <div className="flex items-start gap-2">
              <span className="shrink-0 text-xs font-semibold text-ink-faint">{index + 1}.</span>
              <code className="min-w-0 flex-1 whitespace-pre-wrap break-words text-xs font-semibold text-ink">
                {unit.display}
              </code>
              {unit.outsideWorkspace ? (
                <span className="shrink-0 rounded-full bg-status-warning-soft px-2 py-0.5 text-[10px] font-medium text-status-warning-ink">
                  {t('tool.permission.outsideWorkspace')}
                </span>
              ) : null}
            </div>
            <ul className="mt-2 space-y-1 pl-5">
              {unit.effects.map((effect, effectIndex) => (
                <EffectRow key={effectIndex} display={effect} />
              ))}
              {unit.effects.length === 0 ? (
                <li className="text-xs text-status-warning-ink">{t('tool.permission.unknownEffects')}</li>
              ) : null}
            </ul>
            <div className="mt-2 flex flex-wrap items-center gap-1.5 pl-5 text-[11px] text-ink-faint">
              <span>{verdictLabel(t, unit.verdict)}</span>
              {unit.verdict.ruleId ? <code>{unit.verdict.ruleId}</code> : null}
            </div>
          </li>
        ))}
      </ol>

      {request.card.unparsed ? (
        <div className="mx-4 mb-3 rounded-lg bg-status-warning-soft px-3 py-2 text-xs text-status-warning-ink">
          {t('tool.permission.unparsedWarning')}
        </div>
      ) : null}

      <div className="border-t border-clay-soft px-4 py-3">
        <div className="mb-1 text-[11px] font-medium text-ink-faint">{t('tool.permission.raw')}</div>
        <pre className="max-h-32 overflow-auto whitespace-pre-wrap break-words rounded-lg bg-ink px-3 py-2.5 font-mono text-[11px] leading-relaxed text-paper">
          {request.card.raw}
        </pre>
      </div>

      <div className="flex items-center gap-2 border-t border-clay-soft bg-paper-hover px-4 py-3">
        <button
          type="button"
          disabled={resolving}
          onClick={() => onResolve('allow_once')}
          className="inline-flex min-h-8 items-center gap-1.5 rounded-lg bg-ink px-3.5 py-1.5 text-sm font-medium text-paper transition hover:bg-ink-soft disabled:opacity-50"
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
            className="inline-flex min-h-8 items-center gap-1.5 rounded-lg border border-clay-soft bg-paper px-3.5 py-1.5 text-sm font-medium text-ink transition hover:bg-paper-hover disabled:opacity-50"
          >
            {sessionActionLabel(t, request.card.sessionAction)}
          </button>
        ) : null}
        <div className="flex-1" />
        <button
          type="button"
          disabled={resolving}
          onClick={() => onResolve('deny')}
          className="inline-flex min-h-8 items-center gap-1.5 rounded-lg border border-status-danger-border bg-paper px-3.5 py-1.5 text-sm font-medium text-status-danger-ink transition hover:bg-status-danger-soft disabled:opacity-50"
        >
          <X size={14} />
          {t('tool.reject')}
        </button>
      </div>
    </div>
  )
}

/// 输入框上方的权限卡片。Permission 属于对应 Session Runtime，直到 Core 事件确认已处理。
export function ApprovalDialog({ sessionId }: { sessionId: string | null }) {
  const current = useRuntimeStore((state) =>
    sessionId ? state.bySession[sessionId]?.pendingPermission ?? null : null,
  )
  const { resolvePermission } = useTurnActions(sessionId)
  const [resolving, setResolving] = useState(false)

  if (!current) return null

  return (
    <ApprovalCardView
      request={current}
      resolving={resolving}
      onResolve={(decision) => {
        if (resolving) return
        setResolving(true)
        void resolvePermission(decision).finally(() => setResolving(false))
      }}
    />
  )
}
