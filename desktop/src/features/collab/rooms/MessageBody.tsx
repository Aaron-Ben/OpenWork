import type { ReactNode } from 'react'

import { MarkdownRenderer } from '@/components/markdown/MarkdownRenderer'
import { identityClasses } from '@/features/collab/components/agentIdentity'
import { cn } from '@/lib/utils'
import { CardChip } from './CardLinks'
import { textSegments } from './messageText'
import type { FoundCard } from './roomTimeline'

/**
 * 消息正文：Markdown 照常渲染，代码之外的 `@<id>` 渲染成提及标签、卡片 id 渲染成胶囊
 * （collaboration-desktop.md §7.2）。
 */
export function MessageBody({ body, knownIds, findCard, selectedCardId, onOpenCard }: {
  body: string
  knownIds: ReadonlySet<string>
  findCard: (cardId: string) => FoundCard | null
  selectedCardId: string | null
  onOpenCard: (cardId: string) => void
}) {
  const renderText = (text: string): ReactNode => textSegments(text, knownIds).map((segment, index) => {
    switch (segment.kind) {
      case 'text': return segment.text
      case 'mention':
        return (
          <span key={index} className={cn('rounded px-1 font-semibold', segment.id === 'all' ? 'bg-clay-soft text-clay-ink' : identityClasses(segment.id).mention)}>
            @{segment.id}
          </span>
        )
      case 'card':
        return (
          <CardChip
            key={index}
            cardId={segment.id}
            found={findCard(segment.id)}
            selected={segment.id === selectedCardId}
            onOpen={onOpenCard}
          />
        )
      default: {
        const unreachable: never = segment
        return unreachable
      }
    }
  })
  return <MarkdownRenderer content={body} variant="compact" renderText={renderText} />
}
