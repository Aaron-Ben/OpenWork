import { AtSign, Send, X } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabAgent, CollabRoomMessage } from '@/bridge/collab'
import { resolveErrorMessage } from '@/lib/commandError'
import { useCollabNavigationStore } from '@/features/collab/collabNavigationStore'
import { MentionTextarea } from '@/features/collab/components/MentionTextarea'
import { useMessageStore } from './messageStore'
import { useRoomViewStore } from './roomViewStore'

/** 输入框（collaboration-desktop.md §7.4）：引用条、带 `@` 补全的文本框（Enter 发送）、工具行。 */
export function Composer({ roomId, members }: { roomId: string, members: CollabAgent[] }) {
  const { t } = useTranslation()
  const send = useMessageStore((state) => state.send)
  const quoting = useRoomViewStore((state) => state.quoting)
  const quote = useRoomViewStore((state) => state.quote)
  const followLatest = useRoomViewStore((state) => state.followLatest)
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const [draft, setDraft] = useState('')
  const [error, setError] = useState<string | null>(null)
  const takeDraft = useCollabNavigationStore((state) => state.takeDraft)

  useEffect(() => {
    const seeded = takeDraft(roomId)
    if (seeded === null) return
    setDraft(seeded)
    globalThis.requestAnimationFrame(() => {
      const element = textareaRef.current
      element?.focus()
      element?.setSelectionRange(seeded.length, seeded.length)
    })
  }, [roomId, takeDraft])

  function startMention() {
    const element = textareaRef.current
    const caret = element?.selectionStart ?? draft.length
    const needsSpace = caret > 0 && !/\s/.test(draft[caret - 1] ?? '')
    const text = `${draft.slice(0, caret)}${needsSpace ? ' ' : ''}@${draft.slice(caret)}`
    const nextCaret = caret + (needsSpace ? 2 : 1)
    setDraft(text)
    globalThis.requestAnimationFrame(() => {
      element?.focus()
      element?.setSelectionRange(nextCaret, nextCaret)
    })
  }

  async function submit(event?: React.FormEvent) {
    event?.preventDefault()
    const body = draft.trim()
    if (!body) return
    const quoted = quoting
    setDraft('')
    setError(null)
    quote(null)
    followLatest()
    try {
      await send(roomId, body, quoted?.id ?? null)
    } catch (sendError) {
      // 发送失败时把草稿与引用还给用户，不让输入丢失。
      setDraft(body)
      quote(quoted)
      setError(resolveErrorMessage(sendError))
    }
  }

  return (
    <form className="relative flex flex-col rounded-2xl border border-line-strong bg-surface" onSubmit={(event) => void submit(event)}>
      {quoting ? <QuoteBar quoting={quoting} onCancel={() => quote(null)} /> : null}
      <div className="px-3 pt-2.5">
        <MentionTextarea
          textareaRef={textareaRef}
          value={draft}
          onChange={setDraft}
          agents={members}
          submitKey="enter"
          onSubmit={() => void submit()}
          rows={2}
          label={t('collab.rooms.messageLabel')}
          placeholder={t('collab.rooms.messagePlaceholder')}
          className="text-sm leading-relaxed"
        />
      </div>
      {error ? <p className="px-3 pt-1 text-xs text-status-danger-ink">{error}</p> : null}
      <div className="flex items-center gap-1.5 px-2 pb-2 pl-2.5 pt-1.5">
        <button type="button" aria-label={t('collab.rooms.mentionButton')} className="grid size-[30px] place-items-center rounded-lg text-ink-soft hover:bg-paper-hover" onClick={startMention}>
          <AtSign size={16} />
        </button>
        <span className="flex-1 truncate text-xs text-ink-soft">{t('collab.rooms.composerHint')}</span>
        <button type="submit" disabled={!draft.trim()} className="flex h-8 items-center gap-1.5 rounded-lg bg-ink px-3.5 text-[13px] font-semibold text-paper disabled:opacity-40">
          <Send size={14} />{t('collab.rooms.send')}
        </button>
      </div>
    </form>
  )
}

/** 引用条：正在引用时显示“回复 <名字>：<原文>”与取消按钮（collaboration-desktop.md §7.4）。 */
export function QuoteBar({ quoting, onCancel }: { quoting: CollabRoomMessage, onCancel: () => void }) {
  const { t } = useTranslation()
  const name = quoting.authorKind === 'user' ? t('collab.rooms.user') : quoting.authorName
  return (
    <div className="flex items-center gap-2 border-b border-line px-3 py-1.5 text-xs text-ink-soft">
      <span className="min-w-0 flex-1 truncate">{`${t('collab.rooms.replyingTo', { name })}${quoting.body}`}</span>
      <button type="button" aria-label={t('collab.rooms.cancelQuote')} className="grid size-6 place-items-center rounded text-ink-soft hover:text-ink" onClick={onCancel}>
        <X size={14} />
      </button>
    </div>
  )
}
