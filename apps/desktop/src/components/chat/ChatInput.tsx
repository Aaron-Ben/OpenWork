import { useEffect, useRef, useState } from 'react'
import { CheckCircle2, ChevronDown, Loader2, Send, ShieldCheck } from 'lucide-react'

interface ChatInputProps {
  activeProviderName: string
  model: string
  modelOptions: string[]
  value: string
  isSending: boolean
  disabled?: boolean
  onValueChange: (value: string) => void
  onModelChange: (model: string) => void
  onSubmit: () => void
}

export function ChatInput({
  activeProviderName,
  model,
  modelOptions,
  value,
  isSending,
  disabled = false,
  onValueChange,
  onModelChange,
  onSubmit,
}: ChatInputProps) {
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const permissionRef = useRef<HTMLDivElement>(null)
  const composingRef = useRef(false)
  const [permissionOpen, setPermissionOpen] = useState(false)

  useEffect(() => {
    const textarea = textareaRef.current
    if (!textarea) return
    textarea.style.height = 'auto'
    textarea.style.height = `${Math.min(textarea.scrollHeight, 200)}px`
  }, [value])

  useEffect(() => {
    if (!permissionOpen) return

    function handlePointerDown(event: MouseEvent) {
      if (!permissionRef.current?.contains(event.target as Node)) {
        setPermissionOpen(false)
      }
    }

    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === 'Escape') setPermissionOpen(false)
    }

    document.addEventListener('mousedown', handlePointerDown)
    document.addEventListener('keydown', handleKeyDown)
    return () => {
      document.removeEventListener('mousedown', handlePointerDown)
      document.removeEventListener('keydown', handleKeyDown)
    }
  }, [permissionOpen])

  function handleKeyDown(event: React.KeyboardEvent<HTMLTextAreaElement>) {
    if (composingRef.current || event.nativeEvent.isComposing || event.keyCode === 229) return
    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault()
      onSubmit()
    }
  }

  return (
    <div className="mx-auto w-full max-w-[980px] px-4 pb-7">
      <form
        className="overflow-hidden rounded-[18px] border border-stone-200 bg-white shadow-[0_18px_60px_rgba(15,23,42,0.10)]"
        onSubmit={(event) => {
          event.preventDefault()
          onSubmit()
        }}
      >
        <textarea
          ref={textareaRef}
          className="max-h-48 min-h-[118px] w-full resize-none border-0 bg-transparent px-5 py-5 text-base leading-7 text-slate-900 outline-none placeholder:text-stone-400 focus:ring-0"
          value={value}
          onChange={(event) => onValueChange(event.target.value)}
          onKeyDown={handleKeyDown}
          onCompositionStart={() => {
            composingRef.current = true
          }}
          onCompositionEnd={() => {
            composingRef.current = false
          }}
          placeholder="随便问点什么..."
          rows={4}
          disabled={disabled}
        />

        <div className="mx-5 border-t border-stone-200" />

        <div className="flex flex-nowrap items-center gap-3 px-5 py-3 max-[720px]:flex-wrap">
          <div ref={permissionRef} className="relative">
            <button
              type="button"
              className="inline-flex h-10 items-center gap-2 rounded-full bg-stone-100 px-3 text-sm font-medium text-stone-700 transition hover:bg-stone-200"
              aria-haspopup="menu"
              aria-expanded={permissionOpen}
              aria-label="执行权限: 审批权限"
              onClick={() => setPermissionOpen((open) => !open)}
            >
              <ShieldCheck size={15} className="text-orange-600" />
              <span>审批权限</span>
              <ChevronDown size={14} className="text-stone-400" />
            </button>

            {permissionOpen ? (
              <div
                role="menu"
                className="absolute bottom-full left-0 z-30 mb-2 w-[280px] overflow-hidden rounded-xl border border-stone-200 bg-white py-2 shadow-[0_16px_44px_rgba(15,23,42,0.16)]"
              >
                <div className="px-4 py-2 text-[10px] font-bold uppercase tracking-widest text-stone-400">
                  执行权限
                </div>
                <button
                  type="button"
                  role="menuitem"
                  className="flex w-full items-start gap-3 bg-orange-50 px-4 py-3 text-left"
                  onClick={() => setPermissionOpen(false)}
                >
                  <ShieldCheck size={18} className="mt-0.5 shrink-0 text-orange-600" />
                  <span className="min-w-0 flex-1">
                    <span className="block text-sm font-semibold text-slate-900">审批权限</span>
                    <span className="mt-0.5 block text-xs leading-5 text-stone-500">
                      工具调用前暂停，等待你确认允许或拒绝。
                    </span>
                  </span>
                  <CheckCircle2 size={16} className="mt-0.5 shrink-0 text-orange-600" />
                </button>
              </div>
            ) : null}
          </div>

          <div className="min-w-0 flex-1" />

          {isSending ? (
            <span className="inline-flex h-10 items-center gap-1.5 rounded-full bg-sky-50 px-3 text-sm font-medium text-sky-700">
              <Loader2 size={14} className="animate-spin" /> Streaming
            </span>
          ) : null}

          <span className="inline-flex h-10 max-w-[180px] items-center gap-2 rounded-full border border-stone-200 bg-white px-3 text-sm text-stone-600">
            <span className={`size-2 rounded-full ${disabled ? 'bg-amber-500' : 'bg-emerald-500'}`} />
            <span className="truncate">{activeProviderName}</span>
          </span>

          <select
            className="h-10 max-w-[260px] rounded-full border border-stone-200 bg-stone-100 px-4 text-sm font-semibold text-slate-800 outline-none hover:bg-stone-200"
            value={model}
            onChange={(event) => onModelChange(event.target.value)}
            disabled={isSending}
          >
            {modelOptions.length === 0 ? (
              <option value="">no model</option>
            ) : (
              modelOptions.map((option) => (
                <option key={option} value={option}>
                  {option}
                </option>
              ))
            )}
          </select>

          <button
            className="inline-flex h-11 min-w-[144px] items-center justify-center gap-2 rounded-2xl bg-orange-700 px-5 text-sm font-semibold text-white hover:bg-orange-800 disabled:cursor-not-allowed disabled:bg-stone-300 disabled:text-stone-500"
            type="submit"
            aria-label="Send"
            disabled={disabled || isSending || !model || !value.trim()}
          >
            {isSending ? <Loader2 size={18} className="animate-spin" /> : <Send size={18} />}
            运行
          </button>
        </div>
      </form>
    </div>
  )
}
