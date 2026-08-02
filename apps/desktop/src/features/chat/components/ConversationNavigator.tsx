import { useEffect, useState, type RefObject } from 'react'
import { motion, useReducedMotion } from 'motion/react'
import { useTranslation } from 'react-i18next'

import { cn } from '@/lib/utils'
import type { ChatItem } from '@/types/chat'

export interface ConversationTurn {
  id: string
  label: string
}

export function getMarkerWidth(index: number, hoveredIndex: number | null, active: boolean) {
  if (hoveredIndex === null) return active ? 16 : 12

  const distance = Math.abs(index - hoveredIndex)
  return [52, 40, 28, 20][distance] ?? 12
}

export function getConversationTurns(messages: ChatItem[]): ConversationTurn[] {
  return messages
    .filter((message) => message.role === 'user')
    .map((message, index) => ({
      id: message.turnId ?? message.id,
      label: message.parts.find((part) => part.type === 'text')?.text.trim() || `#${index + 1}`,
    }))
}

interface ConversationNavigatorProps {
  turns: ConversationTurn[]
  scrollContainerRef: RefObject<HTMLDivElement | null>
}

export function ConversationNavigator({
  turns,
  scrollContainerRef,
}: ConversationNavigatorProps) {
  const { t } = useTranslation()
  const [activeTurnId, setActiveTurnId] = useState(turns[0]?.id ?? null)
  const [hoveredIndex, setHoveredIndex] = useState<number | null>(null)
  const reduceMotion = useReducedMotion()
  const turnIds = turns.map((turn) => turn.id)
  const turnKey = turnIds.join('\u0000')

  useEffect(() => {
    const container = scrollContainerRef.current
    if (!container || turns.length === 0) return

    let frame = 0
    const updateActiveTurn = () => {
      cancelAnimationFrame(frame)
      frame = requestAnimationFrame(() => {
        const threshold = container.getBoundingClientRect().top + 96
        let activeId = turns[0].id

        for (const turnId of turnIds) {
          const element = Array.from(
            container.querySelectorAll<HTMLElement>('[data-turn-id]'),
          ).find((candidate) => candidate.dataset.turnId === turnId)
          if (!element || element.getBoundingClientRect().top > threshold) break
          activeId = turnId
        }
        setActiveTurnId(activeId)
      })
    }

    updateActiveTurn()
    container.addEventListener('scroll', updateActiveTurn, { passive: true })
    window.addEventListener('resize', updateActiveTurn)
    return () => {
      cancelAnimationFrame(frame)
      container.removeEventListener('scroll', updateActiveTurn)
      window.removeEventListener('resize', updateActiveTurn)
    }
    // Labels may change while streaming, but listeners only depend on the ordered user-turn IDs.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scrollContainerRef, turnKey])

  function jumpToTurn(turnId: string) {
    const container = scrollContainerRef.current
    const target = Array.from(
      container?.querySelectorAll<HTMLElement>('[data-turn-id]') ?? [],
    ).find((candidate) => candidate.dataset.turnId === turnId)
    if (!container || !target) return

    const top = target.getBoundingClientRect().top - container.getBoundingClientRect().top
    container.scrollTo({ top: container.scrollTop + top - 32, behavior: 'smooth' })
    setActiveTurnId(turnId)
  }

  if (turns.length < 2) return null

  return (
    <nav
      aria-label={t('chat.turnNavigation')}
      className="absolute left-4 top-1/2 z-10 hidden w-14 -translate-y-1/2 flex-col md:flex"
      data-conversation-navigator="true"
      onMouseLeave={() => setHoveredIndex(null)}
    >
      {turns.map((turn, index) => {
        const active = turn.id === activeTurnId
        const hovered = hoveredIndex === index
        const emphasized = hoveredIndex === null ? active : hovered
        const previewId = `turn-preview-${index + 1}`
        return (
          <div
            key={turn.id}
            className="group relative flex h-4 w-14 shrink-0 items-center"
            onMouseEnter={() => setHoveredIndex(index)}
            onFocus={() => setHoveredIndex(index)}
            onBlur={() => setHoveredIndex(null)}
          >
            <motion.button
              type="button"
              data-motion-component="ConversationNavigator"
              initial={false}
              animate={{ width: getMarkerWidth(index, hoveredIndex, active) }}
              transition={
                reduceMotion
                  ? { duration: 0 }
                  : { type: 'spring', stiffness: 360, damping: 30, mass: 0.45 }
              }
              aria-label={t('chat.jumpToTurn', { index: index + 1, title: turn.label })}
              aria-describedby={previewId}
              aria-current={active ? 'location' : undefined}
              className={cn(
                'h-1 shrink-0 rounded-full transition-colors duration-150 focus-visible:outline-none',
                emphasized ? 'bg-ink-soft' : 'bg-line-strong',
              )}
              onClick={() => jumpToTurn(turn.id)}
            />
            <div
              id={previewId}
              role="tooltip"
              className="pointer-events-none invisible absolute left-16 top-1/2 w-64 -translate-y-1/2 translate-x-1 rounded-xl border border-line bg-paper px-3 py-2 opacity-0 shadow-lg transition-[opacity,transform,visibility] duration-200 ease-out group-hover:visible group-hover:translate-x-0 group-hover:opacity-100 group-focus-within:visible group-focus-within:translate-x-0 group-focus-within:opacity-100"
            >
              <div className="font-sans text-[11px] font-medium text-ink-faint">
                {t('chat.turnPreview', { index: index + 1 })}
              </div>
              <div className="mt-1 line-clamp-3 text-sm leading-5 text-ink">{turn.label}</div>
            </div>
          </div>
        )
      })}
    </nav>
  )
}
