import { AtSign, Send, X } from 'lucide-react'
import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { CollabAgent, CollabRoomMessage } from '@/bridge/collab'
import { resolveErrorMessage } from '@/lib/commandError'
import { cn } from '@/lib/utils'
import { activeMention, insertMention, mentionCandidates, type ActiveMention } from './mentionCompletion'
import { useMessageStore } from './messageStore'
import { useRoomViewStore } from './roomViewStore'

/**
 * 输入框（collaboration-desktop.md §7.4）：引用条、带 `@` 补全的文本框、工具行。
 * Enter 发送，Shift+Enter 换行；补全打开时上下键选择、Enter 确认、Esc 关闭。
 */
export function Composer({ roomId, members }: { roomId: string, members: CollabAgent[] }) {
  const { t } = useTranslation()
  const send = useMessageStore((state) => state.send)
  const quoting = useRoomViewStore((state) => state.quoting)
  const quote = useRoomViewStore((state) => state.quote)
  const followLatest = useRoomViewStore((state) => state.followLatest)
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const [draft, setDraft] = useState('')
  const [mention, setMention] = useState<ActiveMention | null>(null)
  const [choice, setChoice] = useState(0)
  const [error, setError] = useState<string | null>(null)
  const candidates = mention ? mentionCandidates(mention.query, members) : []

  function track(text: string, caret: number) {
    const next = activeMention(text, caret)
    setMention(next)
    if (next?.query !== mention?.query) setChoice(0)
  }

  function place(text: string, caret: number) {
    setDraft(text)
    setMention(null)
    globalThis.requestAnimationFrame(() => {
      textareaRef.current?.focus()
      textareaRef.current?.setSelectionRange(caret, caret)
    })
  }

  function accept(id: string) {
    if (!mention) return
    const next = insertMention(draft, mention, id)
    place(next.draft, next.caret)
  }

  function startMention() {
    const element = textareaRef.current
    const caret = element?.selectionStart ?? draft.length
    const needsSpace = caret > 0 && !/\s/.test(draft[caret - 1] ?? '')
    const text = `${draft.slice(0, caret)}${needsSpace ? ' ' : ''}@${draft.slice(caret)}`
    const nextCaret = caret + (needsSpace ? 2 : 1)
    setDraft(text)
    setMention({ start: nextCaret - 1, query: '' })
    setChoice(0)
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
    setMention(null)
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

  function onKeyDown(event: React.KeyboardEvent<HTMLTextAreaElement>) {
    if (candidates.length > 0) {
      if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
        event.preventDefault()
        const step = event.key === 'ArrowDown' ? 1 : -1
        setChoice((current) => (current + step + candidates.length) % candidates.length)
        return
      }
      if (event.key === 'Enter' || event.key === 'Tab') {
        event.preventDefault()
        const picked = candidates[choice] ?? candidates[0]
        if (picked) accept(picked.id)
        return
      }
      if (event.key === 'Escape') {
        event.preventDefault()
        setMention(null)
        return
      }
    }
    if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) {
      event.preventDefault()
      void submit()
    }
  }

  return (
    <form className="relative flex flex-col rounded-2xl border border-line-strong bg-surface" onSubmit={(event) => void submit(event)}>
      {quoting ? <QuoteBar quoting={quoting} onCancel={() => quote(null)} /> : null}
      {candidates.length > 0 ? (
        <ul role="listbox" aria-label={t('collab.rooms.mentionButton')} className="absolute bottom-full left-2 z-20 mb-1 max-h-60 w-72 overflow-y-auto rounded-xl border border-line bg-surface p-1 shadow-lg">
          {candidates.map((candidate, index) => (
            <li key={candidate.id} role="option" aria-selected={index === choice}>
              <button
                type="button"
                className={cn('flex w-full items-baseline gap-2 rounded-lg px-2.5 py-1.5 text-left text-sm', index === choice && 'bg-paper-hover')}
                onMouseDown={(event) => { event.preventDefault(); accept(candidate.id) }}
              >
                <span className="font-semibold">@{candidate.id}</span>
                <span className="truncate text-xs text-ink-faint">
                  {'agent' in candidate
                    ? [candidate.agent.displayName, candidate.agent.role].filter(Boolean).join(' · ')
                    : t('collab.rooms.mentionAll')}
                </span>
              </button>
            </li>
          ))}
        </ul>
      ) : null}
      <label className="flex flex-col px-3 pt-2.5">
        <span className="sr-only">{t('collab.rooms.messageLabel')}</span>
        <textarea
          ref={textareaRef}
          rows={2}
          value={draft}
          placeholder={t('collab.rooms.messagePlaceholder')}
          className="resize-none bg-transparent text-sm leading-relaxed text-ink outline-none placeholder:text-ink-faint"
          onChange={(event) => { setDraft(event.target.value); track(event.target.value, event.target.selectionStart) }}
          onSelect={(event) => track(event.currentTarget.value, event.currentTarget.selectionStart)}
          onKeyDown={onKeyDown}
        />
      </label>
      {error ? <p className="px-3 pt-1 text-xs text-status-danger-ink">{error}</p> : null}
      <div className="flex items-center gap-1.5 px-2 pb-2 pl-2.5 pt-1.5">
        <button type="button" aria-label={t('collab.rooms.mentionButton')} className="grid size-[30px] place-items-center rounded-lg text-ink-soft hover:bg-paper-hover" onClick={startMention}>
          <AtSign size={16} />
        </button>
        <span className="flex-1 truncate text-xs text-ink-faint">{t('collab.rooms.composerHint')}</span>
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
      <button type="button" aria-label={t('collab.rooms.cancelQuote')} className="grid size-6 place-items-center rounded text-ink-faint hover:text-ink" onClick={onCancel}>
        <X size={14} />
      </button>
    </div>
  )
}
