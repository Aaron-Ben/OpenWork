import { useEffect, useRef, useState } from 'react'
import { Check, Copy } from 'lucide-react'
import { useTranslation } from 'react-i18next'

/** 复制成功后图标停留多久再变回去。 */
const COPIED_FEEDBACK_MS = 1_500

interface CopyButtonProps {
  /** 要写进剪贴板的文本。为空时不渲染按钮 —— 复制一段空内容没有意义。 */
  text: string
  /** 附加在按钮上的类名，用于控制悬停显隐等外部布局。 */
  className?: string
  iconSize?: number
}

/**
 * 复制按钮。
 *
 * 剪贴板写入、"已复制"回执、以及卸载时清理计时器这三件事在项目里被重复实现过多次，
 * 这里收敛成一处。计时器必须在卸载时清掉，否则组件消失后回调仍会 setState。
 */
export function CopyButton({ text, className = '', iconSize = 12 }: CopyButtonProps) {
  const { t } = useTranslation()
  const [copied, setCopied] = useState(false)
  const timerRef = useRef<number | null>(null)

  useEffect(() => () => {
    if (timerRef.current !== null) window.clearTimeout(timerRef.current)
  }, [])

  if (!text.trim()) return null

  async function copy() {
    try {
      await navigator.clipboard?.writeText(text)
    } catch {
      // 剪贴板不可用（无权限、非安全上下文）时保持静默：这是个辅助动作，
      // 为它弹一个错误提示比复制不了本身更打扰。
      return
    }
    setCopied(true)
    if (timerRef.current !== null) window.clearTimeout(timerRef.current)
    timerRef.current = window.setTimeout(() => setCopied(false), COPIED_FEEDBACK_MS)
  }

  const label = copied ? t('tool.copied') : t('tool.copy')
  return (
    <button
      type="button"
      data-copy-button="true"
      aria-label={label}
      title={label}
      onClick={() => void copy()}
      className={`grid size-6 shrink-0 place-items-center rounded-md text-ink-faint transition-colors hover:bg-paper-hover hover:text-ink ${className}`}
    >
      {copied
        ? <Check size={iconSize} className="text-status-success" />
        : <Copy size={iconSize} />}
    </button>
  )
}
