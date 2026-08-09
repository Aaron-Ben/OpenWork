import {
  BotMessageSquare,
  CircleStop,
  FilePen,
  FilePlus,
  FileText,
  FolderSearch,
  List,
  ListTodo,
  MessageSquarePlus,
  Search,
  SquareTerminal,
  Timer,
  UsersRound,
  Wrench,
  type LucideIcon,
} from 'lucide-react'

/** 工具图标按效果区分；控制类工具保持独立语义，避免全部退化成扳手。 */
export type ToolIconEffect =
  | 'read'
  | 'write'
  | 'edit'
  | 'grep'
  | 'glob'
  | 'list'
  | 'bash'
  | 'update_plan'
  | 'spawn_agent'
  | 'wait_agent'
  | 'list_agents'
  | 'followup_task'
  | 'interrupt_agent'
  | 'unknown'

const EXACT_EFFECT: Record<string, ToolIconEffect> = {
  read: 'read',
  write: 'write',
  edit: 'edit',
  grep: 'grep',
  glob: 'glob',
  list: 'list',
  bash: 'bash',
  update_plan: 'update_plan',
  spawn_agent: 'spawn_agent',
  wait_agent: 'wait_agent',
  list_agents: 'list_agents',
  followup_task: 'followup_task',
  interrupt_agent: 'interrupt_agent',
}

export function traceToolIconEffect(toolName: string | null | undefined): ToolIconEffect {
  const name = (toolName ?? '').toLowerCase()
  if (!name) return 'unknown'
  const exact = EXACT_EFFECT[name]
  if (exact) return exact
  // 模糊兜底：resolved 名带前后缀（read_file、workspace.read 等）也按效果归类。
  if (name.includes('grep') || name.includes('search')) return 'grep'
  if (name.includes('glob') || name.includes('find')) return 'glob'
  if (name.includes('edit') || name.includes('patch')) return 'edit'
  if (name.includes('write') || name.includes('create')) return 'write'
  if (name.includes('read') || name.includes('open') || name.includes('cat')) return 'read'
  if (name.includes('list') || name === 'ls' || name.includes('tree')) return 'list'
  if (name.includes('bash') || name.includes('shell') || name.includes('terminal') || name.includes('process')) return 'bash'
  return 'unknown'
}

const TRACE_TOOL_ICONS: Record<ToolIconEffect, LucideIcon> = {
  read: FileText,
  write: FilePlus,
  edit: FilePen,
  grep: Search,
  glob: FolderSearch,
  list: List,
  bash: SquareTerminal,
  update_plan: ListTodo,
  spawn_agent: BotMessageSquare,
  wait_agent: Timer,
  list_agents: UsersRound,
  followup_task: MessageSquarePlus,
  interrupt_agent: CircleStop,
  unknown: Wrench,
}

export function TraceToolIcon({
  toolName,
  size = 13,
  className,
}: {
  toolName: string | null | undefined
  size?: number
  className?: string
}) {
  const effect = traceToolIconEffect(toolName)
  const Icon = TRACE_TOOL_ICONS[effect]
  return <Icon size={size} className={className} data-tool-icon={effect} aria-hidden="true" />
}
