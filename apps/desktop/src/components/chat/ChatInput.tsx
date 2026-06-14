import { useEffect, useRef } from 'react'
import { Loader2, Send } from 'lucide-react'

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
  const composingRef = useRef(false)

  useEffect(() => {
    const textarea = textareaRef.current
    if (!textarea) return
    textarea.style.height = 'auto'
    textarea.style.height = `${Math.min(textarea.scrollHeight, 200)}px`
  }, [value])

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
