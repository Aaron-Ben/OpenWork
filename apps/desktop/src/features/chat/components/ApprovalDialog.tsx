import { useEffect, useRef, useState } from 'react'
import type { TFunction } from 'i18next'
import { useTranslation } from 'react-i18next'
import {
  Check,
  FileText,
  FolderTree,
  Pencil,
  ShieldAlert,
  Terminal,
  X,
} from 'lucide-react'

import { useRuntimeStore } from '../runtimeStore'
import { useTurnActions } from '../useTurn'

interface ToolDetails {
  primary: string
  content?: string
}

function extractDetails(toolName: string, input: unknown): ToolDetails {
  const obj = input && typeof input === 'object' ? (input as Record<string, unknown>) : {}
  switch (toolName) {
    case 'bash':
      return { primary: typeof obj.command === 'string' ? obj.command : '' }
    case 'write':
      return {
        primary: typeof obj.path === 'string' ? obj.path : '',
        content: typeof obj.content === 'string' ? obj.content : '',
      }
    case 'read':
    case 'list':
      return { primary: typeof obj.path === 'string' ? obj.path : '' }
    default:
      return { primary: typeof input === 'string' ? input : safeStringify(input) }
  }
}

function safeStringify(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2)
  } catch {
    return String(value)
  }
}

function titleFor(t: TFunction, toolName: string, primary: string): string {
  const fileName = primary ? primary.split('/').pop() || primary : ''
  switch (toolName) {
    case 'bash':
      return t('tool.allowBash')
    case 'write':
      return fileName ? t('tool.allowWrite', { name: fileName }) : t('tool.allowWriteFile')
    case 'read':
      return fileName ? t('tool.allowRead', { name: fileName }) : t('tool.allowReadFile')
    case 'list':
      return fileName ? t('tool.allowList', { name: fileName }) : t('tool.allowListDirectory')
    default:
      return t('tool.allowTool', { name: toolName })
  }
}

/// 输入框上方的权限卡片。Permission 属于对应 Session Runtime，直到 Core 事件确认已处理。
export function ApprovalDialog({ sessionId }: { sessionId: string | null }) {
  const { t } = useTranslation()
  const current = useRuntimeStore((state) =>
    sessionId ? state.bySession[sessionId]?.pendingPermission ?? null : null,
  )
  const { resolvePermission } = useTurnActions(sessionId)
  const [resolving, setResolving] = useState(false)
  const titleRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (current) titleRef.current?.focus()
  }, [current])

  async function resolve(allow: boolean) {
    if (!current || resolving) return
    setResolving(true)
    try {
      await resolvePermission(allow)
    } finally {
      setResolving(false)
    }
  }

  if (!current) return null

  const meta = (() => {
    switch (current.toolName) {
      case 'bash':
        return { icon: Terminal, label: 'Bash', color: 'text-clay' }
      case 'write':
        return { icon: Pencil, label: 'Write', color: 'text-status-success' }
      case 'read':
        return { icon: FileText, label: 'Read', color: 'text-ink-soft' }
      case 'list':
        return { icon: FolderTree, label: 'List', color: 'text-ink-soft' }
      default:
        return { icon: ShieldAlert, label: current.toolName, color: 'text-ink-faint' }
    }
  })()
  const Icon = meta.icon

  const details = extractDetails(current.toolName, current.input)
  const title = titleFor(t, current.toolName, details.primary)
  const showPath = Boolean(details.primary) && current.toolName !== 'bash'
  const showTerminal = current.toolName === 'bash' && Boolean(details.primary)
  const showContent =
    current.toolName === 'write' && typeof details.content === 'string' && details.content.length > 0

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
          void resolve(false)
        }
      }}
    >
      {/* Header */}
      <div className="flex items-center gap-3 bg-clay-soft px-4 py-3">
        <div className="grid size-8 place-items-center rounded-lg bg-paper shadow-sm ring-1 ring-clay-soft">
          <Icon className={meta.color} size={18} />
        </div>
        <div ref={titleRef} id="permission-title" tabIndex={-1} className="min-w-0 flex-1 outline-none">
          <div className="flex flex-wrap items-center gap-2">
            <span className="min-w-0 break-words text-sm font-semibold text-ink">{title}</span>
            <span className="inline-flex shrink-0 items-center gap-1 rounded-full bg-clay-soft px-2 py-0.5 text-[10px] font-bold uppercase tracking-wider text-clay">
              <span className="size-1.5 animate-pulse rounded-full bg-clay" />
              {t('tool.waitingApproval')}
            </span>
          </div>
        </div>
      </div>

      {/* Tool details */}
      <div className="space-y-2 border-t border-clay-soft px-4 py-3">
        {showPath && (
          <div className="flex items-center gap-2 rounded-lg bg-paper-hover px-3 py-2 font-mono text-xs text-ink-soft">
            <FileText className="size-3.5 flex-shrink-0 text-ink-faint" />
            <span className="truncate">{details.primary}</span>
          </div>
        )}

        {showTerminal && (
          <div className="overflow-x-auto rounded-lg bg-ink px-3 py-2.5">
            <pre className="whitespace-pre-wrap break-words font-mono text-[11px] leading-tight text-paper">
              <span className="select-none text-clay">$ </span>
              {details.primary}
            </pre>
          </div>
        )}

        {showContent && (
          <pre className="max-h-52 overflow-auto rounded-lg bg-ink px-3 py-2.5 font-mono text-[11px] leading-tight text-paper">
            {details.content}
          </pre>
        )}

        {!showPath && !showTerminal && !showContent && (
          <pre className="overflow-auto rounded-lg bg-paper-hover px-3 py-2 font-mono text-xs text-ink-soft">
            {details.primary || t('tool.noInput')}
          </pre>
        )}
      </div>

      {/* Action buttons */}
      <div className="flex items-center gap-2 border-t border-clay-soft bg-paper-hover px-4 py-3">
        <button
          type="button"
          disabled={resolving}
          onClick={() => void resolve(true)}
          className="inline-flex min-h-8 items-center gap-1.5 rounded-lg bg-ink px-3.5 py-1.5 text-sm font-medium text-paper transition hover:bg-ink-soft disabled:opacity-50"
        >
          <Check size={14} />
          {resolving ? t('tool.processing') : t('tool.allow')}
        </button>
        <div className="flex-1" />
        <button
          type="button"
          disabled={resolving}
          onClick={() => void resolve(false)}
          className="inline-flex min-h-8 items-center gap-1.5 rounded-lg border border-status-danger-border bg-paper px-3.5 py-1.5 text-sm font-medium text-status-danger-ink transition hover:bg-status-danger-soft disabled:opacity-50"
        >
          <X size={14} />
          {t('tool.reject')}
        </button>
      </div>
    </div>
  )
}
