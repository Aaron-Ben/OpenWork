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
    <form
      className="m-6 mt-0 rounded-lg border border-slate-300 bg-white max-[560px]:m-4 max-[560px]:mt-0"
      onSubmit={(event) => {
        event.preventDefault()
        onSubmit()
      }}
    >
      <div className="flex flex-wrap items-center gap-2 px-3 pt-3 text-xs text-slate-500">
        <span className="rounded-full bg-slate-100 px-2 py-1">{activeProviderName}</span>
        <select
          className="rounded-full border border-slate-200 bg-white px-2 py-1 text-xs text-slate-600 outline-none"
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
        {isSending ? (
          <span className="flex items-center gap-1 rounded-full bg-sky-50 px-2 py-1 text-sky-700">
            <Loader2 size={12} className="animate-spin" /> Streaming
          </span>
        ) : null}
      </div>
      <div className="grid grid-cols-[minmax(0,1fr)_44px] items-end gap-2 p-3">
        <textarea
          ref={textareaRef}
          className="max-h-44 min-h-20 w-full resize-none border-0 bg-transparent leading-6 text-slate-900 outline-none focus:ring-0"
          value={value}
          onChange={(event) => onValueChange(event.target.value)}
          onKeyDown={handleKeyDown}
          onCompositionStart={() => {
            composingRef.current = true
          }}
          onCompositionEnd={() => {
            composingRef.current = false
          }}
          placeholder="Ask Anvil anything..."
          rows={3}
          disabled={disabled}
        />
        <button
          className="grid size-11 place-items-center rounded-lg bg-teal-700 text-white hover:bg-teal-800 disabled:cursor-not-allowed disabled:bg-slate-300"
          type="submit"
          aria-label="Send"
          disabled={disabled || isSending || !model || !value.trim()}
        >
          {isSending ? <Loader2 size={18} className="animate-spin" /> : <Send size={18} />}
        </button>
      </div>
    </form>
  )
}
