/** 消息正文里需要特殊渲染的片段（collaboration-desktop.md §7.2）：`@<id>` 提及与卡片 id。 */
export type TextSegment =
  | { kind: 'text', text: string }
  | { kind: 'mention', id: string }
  | { kind: 'card', id: string }

// 与 Server 的 `routing::mentions` 相同：`@<id>` 前后都不能是 `[A-Za-z0-9_-]`。
const TOKEN_RE = /(?<![A-Za-z0-9_-])@([a-z][a-z0-9-]{0,47})(?![A-Za-z0-9_-])|(?<![A-Za-z0-9_-])(card-[0-9a-f]{32})(?![A-Za-z0-9_-])/g
const CARD_RE = /(?<![A-Za-z0-9_-])card-[0-9a-f]{32}(?![A-Za-z0-9_-])/g
const FENCED_CODE_RE = /^ {0,3}(`{3,}|~{3,})[^\n]*\n[\s\S]*?(?:^ {0,3}\1[^\n]*$|(?![\s\S]))/gm
const INLINE_CODE_RE = /`[^`\n]+`/g

/**
 * 把一段纯文本（不含代码）切成文字、提及与卡片 id。只有 `knownIds` 里的 id 与 `all` 算提及，
 * 其余 `@xxx` 保持原文。
 */
export function textSegments(text: string, knownIds: ReadonlySet<string>): TextSegment[] {
  const segments: TextSegment[] = []
  let plain = ''
  let cursor = 0
  for (const match of text.matchAll(TOKEN_RE)) {
    const [token, mention, card] = match
    const isMention = mention !== undefined && (mention === 'all' || knownIds.has(mention))
    if (!isMention && card === undefined) continue
    plain += text.slice(cursor, match.index)
    if (plain) segments.push({ kind: 'text', text: plain })
    plain = ''
    segments.push(isMention ? { kind: 'mention', id: mention } : { kind: 'card', id: token })
    cursor = match.index + token.length
  }
  plain += text.slice(cursor)
  if (plain) segments.push({ kind: 'text', text: plain })
  return segments
}

/** 正文中代码块与行内代码之外出现的卡片 id，去重后按首次出现排列（摘要卡用）。 */
export function cardIdsOutsideCode(body: string): string[] {
  const prose = body.replace(FENCED_CODE_RE, '').replace(INLINE_CODE_RE, '')
  return [...new Set(prose.match(CARD_RE) ?? [])]
}
