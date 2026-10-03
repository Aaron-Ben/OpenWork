import type {
  RuntimeApprovalCard,
  RuntimeApprovalDanger,
  RuntimePermissionDecision,
} from '@/bridge/compat'

/** 卡片的三种形态（permissions.md §12.1）：越界、危险命令，或两者同时。 */
export type ApprovalKind = 'escalation' | 'danger' | 'combined'

export function approvalKind(card: RuntimeApprovalCard): ApprovalKind {
  if (card.danger && card.paths.length > 0) return 'combined'
  return card.danger ? 'danger' : 'escalation'
}

/**
 * 回车对应的按钮。涉及批量删除时默认是拒绝，与原型一致：删掉的文件无法恢复，
 * 手滑的代价比多点一次高得多。
 */
export function primaryDecision(card: RuntimeApprovalCard): RuntimePermissionDecision {
  return card.danger ? 'deny' : 'allow_once'
}

export interface CommandSegment {
  text: string
  highlighted: boolean
}

/**
 * 把命令按命中片段切开。偏移是 UTF-16 码元，与 JS 字符串下标一致；越界的偏移
 * （不该出现）按整条命令不高亮处理，而不是切出错位的片段。
 */
export function commandSegments(
  command: string,
  danger: RuntimeApprovalDanger | null,
): CommandSegment[] {
  if (!danger || danger.start < 0 || danger.end > command.length || danger.start >= danger.end) {
    return [{ text: command, highlighted: false }]
  }
  return [
    { text: command.slice(0, danger.start), highlighted: false },
    { text: command.slice(danger.start, danger.end), highlighted: true },
    { text: command.slice(danger.end), highlighted: false },
  ].filter((segment) => segment.text.length > 0)
}

/** 命中片段本身，卡片上"为什么要问"一栏显示它。 */
export function dangerText(command: string | null, danger: RuntimeApprovalDanger): string {
  return command?.slice(danger.start, danger.end) ?? ''
}

/**
 * 工作区内的路径显示成"工作区名/相对路径"，其余显示绝对路径（agent-context.md §1 的
 * 同一约定）。`workspaceRoot` 缺失时原样显示。
 */
export function approvalPathLabel(path: string, workspaceRoot?: string): string {
  if (!workspaceRoot) return path
  const root = workspaceRoot.endsWith('/') ? workspaceRoot.slice(0, -1) : workspaceRoot
  const name = root.slice(root.lastIndexOf('/') + 1)
  if (path === root) return name
  return path.startsWith(`${root}/`) ? `${name}/${path.slice(root.length + 1)}` : path
}
