import type { ChatItem } from '@/types/chat'

/**
 * 相邻两条 transcript 消息之间的垂直间距。
 *
 * 间距必须由"上下两个块是什么"决定，不能由消息边界决定。模型一轮回复里发几个工具
 * 调用纯属 provider 的分包方式，用户看不见也不该看见 —— 让它决定间距，同一串工具行
 * 就会被切成远近不等的簇（同消息内 2px，跨消息 24px）。
 *
 * 三档的数值刻意与 AssistantMessage / ToolActivityList 内部的间距一一对应，
 * 这样一段内容无论落在一条消息里还是被拆成几条，看起来完全一样：
 *
 *   tight   ← ToolActivityList 的 `space-y-0.5`
 *   block   ← AssistantMessage 的 `gap-1.5`
 *   section ← 换角色 / 换 Turn 的分隔
 *
 * 前提是消息两端不能再有自己的 padding，否则最小档垫不到 2px。AssistantMessage 和
 * ToolActivityList 的根节点因此都不带 `py-*`，纵向节奏只在这里定义。
 */
export type TranscriptGap = 'none' | 'tight' | 'block' | 'section'

const GAP_CLASS: Record<TranscriptGap, string> = {
  none: '',
  tight: 'mt-0.5',
  block: 'mt-1.5',
  section: 'mt-4',
}

export function transcriptGapClass(gap: TranscriptGap): string {
  return GAP_CLASS[gap]
}

function hasToolRows(item: ChatItem): boolean {
  return item.parts.some((part) => part.type === 'tool_call' || part.type === 'tool_result')
}

/** 与 AssistantMessage 的 showText 同构：非工具 part 一律排在工具行前面。 */
function opensWithProse(item: ChatItem): boolean {
  if (item.model) return true
  if (item.parts.length === 0) return item.isStreaming === true
  return item.parts.some((part) => part.type !== 'tool_call' && part.type !== 'tool_result')
}

/** 计划卡钉在消息末尾，它之后就不再是工具行了。 */
function closesWithToolRows(item: ChatItem): boolean {
  return !item.plan && hasToolRows(item)
}

export function transcriptGap(previous: ChatItem | undefined, current: ChatItem): TranscriptGap {
  if (!previous) return 'none'
  if (previous.role === 'user' || current.role === 'user') return 'section'
  if (!previous.turnId || previous.turnId !== current.turnId) return 'section'
  // 文件汇总卡是独立的一块，不参与工具行的节奏。
  if (current.fileChangePresentation === 'summary') return 'section'
  if (opensWithProse(current) || !hasToolRows(current)) return 'section'
  return closesWithToolRows(previous) ? 'tight' : 'block'
}
