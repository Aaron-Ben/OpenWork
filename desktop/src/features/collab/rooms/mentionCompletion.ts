import type { CollabAgent } from '@/bridge/collab'

/** 光标前正在输入的提及：`@` 的位置与已输入的部分。 */
export interface ActiveMention {
  start: number
  query: string
}

/** `@all` 或一个 Agent（collaboration-desktop.md §7.4）。 */
export type MentionCandidate =
  | { id: 'all' }
  | { id: string, agent: CollabAgent }

/** 光标紧跟在 `@` 词后面时返回它；`@` 前是字母数字等 id 字符时不算（与消息里的提及规则一致）。 */
export function activeMention(draft: string, caret: number): ActiveMention | null {
  const match = /(?<![A-Za-z0-9_-])@([a-z0-9-]*)$/i.exec(draft.slice(0, caret))
  if (!match) return null
  return { start: match.index, query: match[1] ?? '' }
}

/** 候选依次为 `@all` 与房间里未归档的 Agent，按 id 或显示名的前缀匹配（不区分大小写）。 */
export function mentionCandidates(query: string, agents: CollabAgent[]): MentionCandidate[] {
  const needle = query.toLowerCase()
  const matching = agents.filter((agent) => agent.archivedAt === null
    && (agent.id.startsWith(needle) || agent.displayName.toLowerCase().startsWith(needle)))
  const all: MentionCandidate[] = 'all'.startsWith(needle) ? [{ id: 'all' }] : []
  return [...all, ...matching.map((agent) => ({ id: agent.id, agent }))]
}

/** 用 `@<id> ` 替换正在输入的提及，返回新文本与光标位置。 */
export function insertMention(
  draft: string,
  mention: ActiveMention,
  id: string,
): { draft: string, caret: number } {
  const end = mention.start + 1 + mention.query.length
  const inserted = `@${id} `
  return {
    draft: draft.slice(0, mention.start) + inserted + draft.slice(end),
    caret: mention.start + inserted.length,
  }
}
