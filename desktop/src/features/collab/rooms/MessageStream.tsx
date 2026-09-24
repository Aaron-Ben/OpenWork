import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabAgent, CollabRoomMessage } from '@/bridge/collab'
import { useBoardStore } from '@/features/collab/boards/boardStore'
import { ScrollToLatestButton } from '@/features/collab/components/ScrollToLatestButton'
import { MessageItem, type MessageContext } from './MessageItem'
import { useMessageStore } from './messageStore'
import { RoomNoteRow } from './RoomNoteRow'
import { findCard, roomTimeline } from './roomTimeline'
import { useRoomViewStore } from './roomViewStore'

/** 离底部不到这么多像素时视为停在最新消息处。 */
const AT_BOTTOM_THRESHOLD_PX = 72
/** 跳回原消息后高亮多久。 */
const HIGHLIGHT_MS = 1_600

/**
 * 房间的消息流（collaboration-desktop.md §7.2、§7.3）：打开时加载快照，停在底部时跟随新消息，
 * 回到前台时补报已看到的位置；点击引用跳回原消息并短暂高亮。
 */
export function MessageStream({ roomId, agents, onQuote }: {
  roomId: string
  agents: CollabAgent[]
  onQuote: ((message: CollabRoomMessage) => void) | null
}) {
  const { t } = useTranslation()
  const roomWindow = useMessageStore((state) => state.byRoom[roomId])
  const open = useMessageStore((state) => state.open)
  const markViewed = useMessageStore((state) => state.markViewed)
  const boards = useBoardStore((state) => state.boards)
  const panel = useRoomViewStore((state) => state.panel)
  const highlightedMessageId = useRoomViewStore((state) => state.highlightedMessageId)
  const highlight = useRoomViewStore((state) => state.highlight)
  const showCard = useRoomViewStore((state) => state.showCard)
  const showAgent = useRoomViewStore((state) => state.showAgent)
  const followToken = useRoomViewStore((state) => state.followToken)
  const scrollAreaRef = useRef<HTMLDivElement>(null)
  const [atBottom, setAtBottom] = useState(true)
  const snapshot = roomWindow?.snapshot ?? null
  const messages = snapshot?.messages ?? []

  useEffect(() => {
    setAtBottom(true)
    void open(roomId)
  }, [open, roomId])

  useEffect(() => {
    const onForeground = () => void markViewed(roomId)
    globalThis.addEventListener('focus', onForeground)
    document.addEventListener('visibilitychange', onForeground)
    return () => {
      globalThis.removeEventListener('focus', onForeground)
      document.removeEventListener('visibilitychange', onForeground)
    }
  }, [markViewed, roomId])

  useEffect(() => {
    if (!highlightedMessageId) return
    const timer = globalThis.setTimeout(() => highlight(null), HIGHLIGHT_MS)
    return () => globalThis.clearTimeout(timer)
  }, [highlight, highlightedMessageId])

  useEffect(() => {
    if (followToken > 0) setAtBottom(true)
  }, [followToken])

  const latestSequence = messages[messages.length - 1]?.sequence ?? 0
  useLayoutEffect(() => {
    if (!atBottom) return
    const frame = globalThis.requestAnimationFrame(() => scrollToLatest('auto'))
    return () => globalThis.cancelAnimationFrame(frame)
  }, [atBottom, latestSequence, roomId])

  function scrollToLatest(behavior: ScrollBehavior = 'smooth') {
    const element = scrollAreaRef.current
    if (!element) return
    element.scrollTo({ top: element.scrollHeight, behavior })
    setAtBottom(true)
  }

  function jump(messageId: string) {
    const target = scrollAreaRef.current?.querySelector<HTMLElement>(`[data-message-id="${CSS.escape(messageId)}"]`)
    if (!target) return
    target.scrollIntoView({ behavior: 'smooth', block: 'center' })
    highlight(messageId)
  }

  const context: MessageContext = {
    knownIds: new Set(agents.map((agent) => agent.id)),
    agentNames: new Map(agents.map((agent) => [agent.id, agent.displayName])),
    findCard: (cardId) => findCard(boards, cardId),
    selectedCardId: panel.kind === 'card' ? panel.cardId : null,
    highlightedMessageId,
    onOpenCard: showCard,
    onOpenAgent: showAgent,
    onJump: jump,
    onQuote,
  }

  return (
    <div className="relative min-h-0 flex-1">
      <div
        ref={scrollAreaRef}
        className="h-full overflow-y-auto px-6 pb-2 pt-4"
        onScroll={(event) => {
          const element = event.currentTarget
          setAtBottom(element.scrollHeight - element.scrollTop - element.clientHeight < AT_BOTTOM_THRESHOLD_PX)
        }}
      >
        <div className="flex flex-col gap-3">
          {roomTimeline(messages, snapshot?.notes ?? []).map((item) => item.kind === 'message'
            ? <MessageItem key={item.key} message={item.message} context={context} />
            : <RoomNoteRow key={item.key} note={item.note} />)}
        </div>
        {roomWindow?.loading ? <p className="py-4 text-center text-sm text-ink-faint">{t('collab.rooms.loading')}</p> : null}
        {snapshot && messages.length === 0 ? <p className="py-16 text-center text-sm text-ink-faint">{t('collab.rooms.noMessages')}</p> : null}
        {roomWindow?.error ? <p className="py-2 text-sm text-status-danger-ink">{roomWindow.error}</p> : null}
      </div>
      <ScrollToLatestButton visible={!atBottom} label={t('collab.rooms.scrollToLatest')} onClick={() => scrollToLatest()} />
    </div>
  )
}
