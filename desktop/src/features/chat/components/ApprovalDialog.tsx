import { useEffect, useRef, useState } from 'react'
import type { TFunction } from 'i18next'
import { useTranslation } from 'react-i18next'
import { Check, CircleAlert, ShieldAlert, ShieldCheck, Trash2, X } from 'lucide-react'

import type {
  RuntimeApprovalCard,
  RuntimeApprovalDanger,
  RuntimeApprovalPath,
  RuntimePermissionDecision,
  RuntimePermissionRequest,
} from '@/bridge/compat'
import { cn } from '@/lib/utils'
import {
  approvalKind,
  approvalPathLabel,
  commandSegments,
  dangerText,
  primaryDecision,
  type ApprovalKind,
} from '../approvalCard'
import { useRuntimeStore } from '../runtimeStore'
import { useTurnActions } from '../useTurn'

interface ApprovalCardViewProps {
  request: RuntimePermissionRequest
  resolving: boolean
  onResolve: (decision: RuntimePermissionDecision) => void
  workspaceRoot?: string
}

const SECTION_LABEL = 'text-xs font-semibold text-ink-soft'
const MONO = 'font-mono text-[12.5px]'

function modeName(t: TFunction, card: RuntimeApprovalCard): string {
  return t(`chat.permissionModes.${card.mode}`)
}

function title(t: TFunction, request: RuntimePermissionRequest, kind: ApprovalKind): string {
  const count = request.card.paths.length
  switch (kind) {
    case 'danger':
      return t('tool.permission.dangerTitle')
    case 'combined':
      return t('tool.permission.combinedTitle', { count })
    case 'escalation':
      return request.card.command === null
        ? t('tool.permission.fileEscalationTitle', { count })
        : t('tool.permission.escalationTitle', { count })
    default: {
      const unreachable: never = kind
      return unreachable
    }
  }
}

function grantList(
  t: TFunction,
  paths: RuntimeApprovalPath[],
  workspaceRoot?: string,
): string {
  const grants = paths.map((path) =>
    t('tool.permission.grant', {
      access: t(`tool.permission.${path.access}`),
      path: approvalPathLabel(path.path, workspaceRoot),
    }),
  )
  return grants.join(t('tool.permission.listSeparator'))
}

function CommandBlock({ command, danger }: { command: string; danger: RuntimeApprovalDanger | null }) {
  const { t } = useTranslation()
  return (
    <div className="flex flex-col gap-1.5">
      <div className={SECTION_LABEL}>
        {danger ? t('tool.permission.fullCommand') : t('tool.permission.command')}
      </div>
      <pre className={cn('m-0 whitespace-pre-wrap rounded-lg bg-code-bg px-3 py-2.5 leading-relaxed text-ink', MONO)}>
        {commandSegments(command, danger).map((segment, index) =>
          segment.highlighted ? (
            <mark
              key={index}
              className="rounded-sm border-b-2 border-status-danger bg-status-danger-soft px-0.5 text-status-danger-ink"
            >
              {segment.text}
            </mark>
          ) : (
            <span key={index}>{segment.text}</span>
          ),
        )}
      </pre>
    </div>
  )
}

function AccessBadge({ access }: { access: RuntimeApprovalPath['access'] }) {
  const { t } = useTranslation()
  return (
    <span
      className={cn(
        'shrink-0 rounded-md border px-2 py-0.5 text-[11px] font-semibold',
        access === 'write'
          ? 'border-status-danger-border bg-status-danger-soft text-status-danger-ink'
          : 'border-status-success-border bg-status-success-soft text-status-success-ink',
      )}
    >
      {t(`tool.permission.${access}`)}
    </span>
  )
}

function TierBadge({ path }: { path: RuntimeApprovalPath }) {
  const { t } = useTranslation()
  const tier = path.tier === 'normal' && path.inWorkspace ? 'workspace' : path.tier
  const tone = path.tier === 'credential' || path.tier === 'hard_protected'
    ? 'bg-status-danger-soft text-status-danger-ink'
    : path.tier === 'sensitive'
      ? 'bg-status-warning-soft text-status-warning-ink'
      : 'bg-paper-hover text-ink-soft'
  return (
    <span className={cn('shrink-0 rounded-full px-2 py-0.5 text-[11px]', tone)}>
      {t(`tool.permission.tier.${tier}`)}
    </span>
  )
}

function PathList({ paths, workspaceRoot }: { paths: RuntimeApprovalPath[]; workspaceRoot?: string }) {
  const { t } = useTranslation()
  const coversGit = paths.some((path) => path.tier === 'sensitive' && /\/\.git(\/|$)/i.test(path.path))
  return (
    <div className="flex flex-col gap-1.5">
      <div className={SECTION_LABEL}>{t('tool.permission.extraPaths')}</div>
      <ul className="m-0 flex list-none flex-col overflow-hidden rounded-[10px] border border-line p-0">
        {paths.map((path, index) => (
          <li
            key={`${path.access}:${path.path}`}
            className={cn('flex items-center gap-2.5 bg-surface px-3 py-2.5', index > 0 && 'border-t border-line')}
          >
            <AccessBadge access={path.access} />
            <code className={cn('min-w-0 grow break-all text-ink', MONO)}>
              {approvalPathLabel(path.path, workspaceRoot)}
            </code>
            <span className="shrink-0 text-xs text-ink-soft">{t(`tool.permission.${path.scope}`)}</span>
            <TierBadge path={path} />
          </li>
        ))}
      </ul>
      {coversGit ? <div className="text-xs text-ink-soft">{t('tool.permission.hooksNote')}</div> : null}
    </div>
  )
}

function DangerReason({ card }: { card: RuntimeApprovalCard }) {
  const { t } = useTranslation()
  if (!card.danger) return null
  return (
    <div className="flex flex-col gap-1.5">
      <div className={SECTION_LABEL}>{t('tool.permission.whyAsk')}</div>
      <div className="flex flex-wrap items-center gap-2.5 rounded-[10px] border border-line bg-surface px-3 py-2.5">
        <code className={cn('text-status-danger-ink', MONO)}>{dangerText(card.command, card.danger)}</code>
        <span className="text-xs text-ink-soft">{t(`tool.permission.dangerKeys.${card.danger.key}`)}</span>
        <code className="ml-auto font-mono text-[11px] text-ink-soft">{card.danger.key}</code>
      </div>
    </div>
  )
}

function Note({ tone, children }: { tone: 'warning' | 'sandbox'; children: React.ReactNode }) {
  const Icon = tone === 'warning' ? CircleAlert : ShieldCheck
  return (
    <div
      className={cn(
        'flex items-start gap-2 rounded-lg px-3 py-2.5 text-xs leading-relaxed',
        tone === 'warning'
          ? 'bg-status-warning-soft text-status-warning-ink'
          : 'bg-status-success-soft text-status-success-ink',
      )}
    >
      <Icon size={14} className="mt-0.5 shrink-0" />
      <span>{children}</span>
    </div>
  )
}

function footerText(
  t: TFunction,
  card: RuntimeApprovalCard,
  kind: ApprovalKind,
  workspaceRoot?: string,
): string {
  const mode = modeName(t, card)
  switch (kind) {
    case 'danger':
      return t('tool.permission.dangerFooter')
    case 'combined':
      return t('tool.permission.combinedFooter', { grants: grantList(t, card.paths, workspaceRoot) })
    case 'escalation':
      return t('tool.permission.escalationFooter', { grants: grantList(t, card.paths, workspaceRoot), mode })
    default: {
      const unreachable: never = kind
      return unreachable
    }
  }
}

export function ApprovalCardView({ request, resolving, onResolve, workspaceRoot }: ApprovalCardViewProps) {
  const { t } = useTranslation()
  const primaryRef = useRef<HTMLButtonElement>(null)
  const { card } = request
  const kind = approvalKind(card)
  const primary = primaryDecision(card)
  const dangerous = primary === 'deny'
  const HeaderIcon = kind === 'escalation' ? ShieldAlert : Trash2

  useEffect(() => {
    primaryRef.current?.focus()
  }, [request.toolCallId])

  function handleKeyDown(event: React.KeyboardEvent<HTMLElement>) {
    if (resolving) return
    if (event.key === 'Escape') {
      event.preventDefault()
      onResolve('deny')
      return
    }
    if (event.key !== 'Enter' || (event.target as HTMLElement).closest('button')) return
    event.preventDefault()
    onResolve(primary)
  }

  const allowButton = (
    <button
      key="allow"
      ref={dangerous ? undefined : primaryRef}
      type="button"
      disabled={resolving}
      onClick={() => onResolve('allow_once')}
      className={cn(
        'inline-flex min-h-9 shrink-0 items-center gap-1.5 whitespace-nowrap rounded-lg px-4 text-sm font-medium transition disabled:opacity-50',
        dangerous
          ? 'border border-status-danger-border bg-paper text-status-danger-ink hover:bg-paper-hover'
          : 'bg-clay text-paper hover:opacity-90',
      )}
    >
      {dangerous ? null : <Check size={14} />}
      {resolving ? t('tool.processing') : t('tool.permission.allowOnce')}
    </button>
  )
  const denyButton = (
    <button
      key="deny"
      ref={dangerous ? primaryRef : undefined}
      type="button"
      disabled={resolving}
      onClick={() => onResolve('deny')}
      className={cn(
        'inline-flex min-h-9 shrink-0 items-center gap-1.5 whitespace-nowrap rounded-lg px-4 text-sm font-medium transition disabled:opacity-50',
        dangerous ? 'bg-clay text-paper hover:opacity-90' : 'ml-auto text-ink-soft hover:bg-paper-hover',
      )}
    >
      <X size={14} />
      {t('tool.reject')}
    </button>
  )

  return (
    <section
      aria-labelledby={`approval-title-${request.toolCallId}`}
      onKeyDown={handleKeyDown}
      className={cn(
        'mx-auto mb-3 flex w-full max-w-3xl flex-col overflow-hidden rounded-[14px] border bg-paper shadow-sm',
        dangerous ? 'border-status-danger-border' : 'border-clay/45',
      )}
    >
      <header className={cn('flex items-center gap-3 px-4 py-3.5', dangerous ? 'bg-status-danger-soft' : 'bg-clay-soft')}>
        <div className="grid size-[34px] place-items-center rounded-[9px] bg-paper">
          <HeaderIcon size={18} className={dangerous ? 'text-status-danger-ink' : 'text-clay'} />
        </div>
        <div className="flex grow flex-col gap-0.5">
          <h2 id={`approval-title-${request.toolCallId}`} className="m-0 text-[15px] font-semibold text-ink">
            {title(t, request, kind)}
          </h2>
          <div className={cn('text-xs', kind === 'danger' ? 'text-status-danger-ink' : 'text-ink-soft')}>
            {kind === 'danger'
              ? t('tool.permission.dangerSubtitle')
              : t('tool.permission.context', { tool: request.toolName, mode: modeName(t, card) })}
          </div>
        </div>
      </header>

      <div className="flex flex-col gap-4 px-4 py-4">
        {card.command !== null ? <CommandBlock command={card.command} danger={card.danger} /> : null}
        {card.justification ? (
          <div className="flex flex-col gap-1.5">
            <div className={SECTION_LABEL}>{t('tool.permission.justification')}</div>
            <p className="m-0 font-serif text-[14.5px] leading-relaxed text-ink">{card.justification}</p>
          </div>
        ) : null}
        {card.paths.length > 0 ? <PathList paths={card.paths} workspaceRoot={workspaceRoot} /> : null}
        <DangerReason card={card} />
        {card.previousDenial ? (
          <Note tone="warning">{t('tool.permission.previousDenial', { line: card.previousDenial })}</Note>
        ) : null}
        {kind === 'danger' ? (
          <Note tone="sandbox">{t('tool.permission.stillSandboxed', { mode: modeName(t, card) })}</Note>
        ) : null}
      </div>

      <footer className="flex flex-col gap-2.5 border-t border-line bg-code-bg px-4 pb-3.5 pt-3">
        <div className="text-xs text-ink-soft">{footerText(t, card, kind, workspaceRoot)}</div>
        <div className="flex items-center gap-2">
          {dangerous ? [denyButton, allowButton] : [allowButton, denyButton]}
        </div>
        <p className="m-0 text-[11px] text-ink-faint">
          {dangerous ? t('tool.permission.dangerShortcuts') : t('tool.permission.shortcuts')}
        </p>
      </footer>
    </section>
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
  const { resolvePermission } = useTurnActions(sessionId)
  const [resolving, setResolving] = useState(false)

  if (!current) return null

  return (
    <ApprovalCardView
      request={current}
      resolving={resolving}
      workspaceRoot={workspaceRoot}
      onResolve={(decision) => {
        if (resolving) return
        setResolving(true)
        void resolvePermission(decision).finally(() => setResolving(false))
      }}
    />
  )
}
