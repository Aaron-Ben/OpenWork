import { useState } from 'react'
import {
  Check,
  FileText,
  FolderTree,
  Pencil,
  ShieldAlert,
  Terminal,
  X,
} from 'lucide-react'

import { providersApi } from '../../api/providers'
import { useApprovalStore } from '../../stores/approvalStore'
import { useSessionStore } from '../../stores/sessionStore'

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

function titleFor(toolName: string, primary: string): string {
  const fileName = primary ? primary.split('/').pop() || primary : ''
  switch (toolName) {
    case 'bash':
      return '允许执行 Bash 命令'
    case 'write':
      return fileName ? `允许写入 ${fileName}` : '允许写入文件'
    case 'read':
      return fileName ? `允许读取 ${fileName}` : '允许读取文件'
    case 'list':
      return fileName ? `允许列出 ${fileName}` : '允许列出目录'
    default:
      return `允许工具 ${toolName}`
  }
}

/// 内联审批卡片:只渲染当前活跃 session 的 pending;允许/拒绝后回传 resolve_approval。
export function ApprovalDialog() {
  const pending = useApprovalStore((state) => state.pending)
  const remove = useApprovalStore((state) => state.remove)
  const activeSessionId = useSessionStore((state) => state.activeSessionId)
  const [resolving, setResolving] = useState(false)
  const current = activeSessionId
    ? pending.find((item) => item.sessionId === activeSessionId) ?? null
    : null

  async function resolve(allow: boolean) {
    if (!current || resolving) return
    setResolving(true)
    try {
      await providersApi.resolveApproval(current.turnId, current.id, allow)
    } catch {
      // 回传失败(id 已过期 / 通道关闭)也移除本地条目,避免 UI 卡住。
    } finally {
      remove(current.id)
      setResolving(false)
    }
  }

  if (!current) return null

  const meta = (() => {
    switch (current.toolName) {
      case 'bash':
        return { icon: Terminal, label: 'Bash', color: 'text-clay' }
      case 'write':
        return { icon: Pencil, label: 'Write', color: 'text-emerald-500' }
      case 'read':
        return { icon: FileText, label: 'Read', color: 'text-sky-500' }
      case 'list':
        return { icon: FolderTree, label: 'List', color: 'text-sky-500' }
      default:
        return { icon: ShieldAlert, label: current.toolName, color: 'text-ink-faint' }
    }
  })()
  const Icon = meta.icon

  const details = extractDetails(current.toolName, current.input)
  const title = titleFor(current.toolName, details.primary)
  const showPath = Boolean(details.primary) && current.toolName !== 'bash'
  const showTerminal = current.toolName === 'bash' && Boolean(details.primary)
  const showContent =
    current.toolName === 'write' && typeof details.content === 'string' && details.content.length > 0

  return (
    <div className="mb-4 overflow-hidden rounded-lg border border-clay-soft bg-paper shadow-sm">
      {/* Header */}
      <div className="flex items-center gap-3 bg-clay-soft px-4 py-3">
        <div className="grid size-8 place-items-center rounded-lg bg-paper shadow-sm ring-1 ring-clay-soft">
          <Icon className={meta.color} size={18} />
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="min-w-0 break-words text-sm font-semibold text-ink">{title}</span>
            <span className="inline-flex shrink-0 items-center gap-1 rounded-full bg-clay-soft px-2 py-0.5 text-[10px] font-bold uppercase tracking-wider text-clay">
              <span className="size-1.5 animate-pulse rounded-full bg-clay" />
              等待审批
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
              <span className="select-none text-emerald-400">$ </span>
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
            {details.primary || '(no input)'}
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
          {resolving ? '处理中...' : '允许'}
        </button>
        <div className="flex-1" />
        <button
          type="button"
          disabled={resolving}
          onClick={() => void resolve(false)}
          className="inline-flex min-h-8 items-center gap-1.5 rounded-lg border border-red-200 bg-paper px-3.5 py-1.5 text-sm font-medium text-red-600 transition hover:bg-red-50 disabled:opacity-50"
        >
          <X size={14} />
          拒绝
        </button>
      </div>
    </div>
  )
}
