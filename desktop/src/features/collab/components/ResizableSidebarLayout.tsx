import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from 'react'

import { cn } from '@/lib/utils'

interface ResizableSidebarLayoutProps {
  storageKey: string
  defaultWidth: number
  minWidth: number
  maxWidth: number
  resizeLabel: string
  sidebar: ReactNode
  children: ReactNode
  className?: string
}

export function ResizableSidebarLayout({
  storageKey,
  defaultWidth,
  minWidth,
  maxWidth,
  resizeLabel,
  sidebar,
  children,
  className,
}: ResizableSidebarLayoutProps) {
  const persistedKey = `openwork.collab.pane.${storageKey}`
  const clamp = useCallback(
    (value: number) => Math.max(minWidth, Math.min(maxWidth, Math.round(value))),
    [maxWidth, minWidth],
  )
  const [width, setWidth] = useState(() => {
    try {
      const persisted = globalThis.localStorage?.getItem(persistedKey)
      const value = persisted ? Number(persisted) : Number.NaN
      return Number.isFinite(value) ? clamp(value) : defaultWidth
    } catch {
      return defaultWidth
    }
  })
  const widthRef = useRef(width)
  const stopDraggingRef = useRef<(() => void) | null>(null)
  widthRef.current = width

  useEffect(() => {
    try {
      globalThis.localStorage?.setItem(persistedKey, String(width))
    } catch {
      // The layout still works when persistence is unavailable.
    }
  }, [persistedKey, width])

  useEffect(() => () => stopDraggingRef.current?.(), [])

  const resizeBy = useCallback((delta: number) => {
    setWidth((current) => clamp(current + delta))
  }, [clamp])

  const startDragging = useCallback((event: ReactPointerEvent<HTMLDivElement>) => {
    event.preventDefault()
    stopDraggingRef.current?.()
    const startX = event.clientX
    const startWidth = widthRef.current
    const previousCursor = document.body.style.cursor
    const previousUserSelect = document.body.style.userSelect

    const onMove = (moveEvent: PointerEvent) => {
      setWidth(clamp(startWidth + moveEvent.clientX - startX))
    }
    const stop = () => {
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', stop)
      window.removeEventListener('pointercancel', stop)
      document.body.style.cursor = previousCursor
      document.body.style.userSelect = previousUserSelect
      stopDraggingRef.current = null
    }
    stopDraggingRef.current = stop
    document.body.style.cursor = 'col-resize'
    document.body.style.userSelect = 'none'
    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', stop)
    window.addEventListener('pointercancel', stop)
  }, [clamp])

  return (
    <div
      className={cn('grid min-h-0 min-w-0 overflow-hidden', className)}
      style={{ gridTemplateColumns: `${width}px minmax(0, 1fr)` }}
    >
      <div className="relative min-h-0 min-w-0">
        {sidebar}
        <div
          role="separator"
          aria-label={resizeLabel}
          aria-orientation="vertical"
          aria-valuemax={maxWidth}
          aria-valuemin={minWidth}
          aria-valuenow={width}
          className="group absolute -right-1 top-0 z-20 h-full w-2 cursor-col-resize touch-none outline-none"
          tabIndex={0}
          onDoubleClick={() => setWidth(clamp(defaultWidth))}
          onKeyDown={(event) => {
            if (event.key === 'ArrowLeft') resizeBy(-16)
            else if (event.key === 'ArrowRight') resizeBy(16)
            else if (event.key === 'Home') setWidth(minWidth)
            else if (event.key === 'End') setWidth(maxWidth)
            else return
            event.preventDefault()
          }}
          onPointerDown={startDragging}
        >
          <span className="absolute left-1/2 top-0 h-full w-px -translate-x-1/2 bg-transparent transition-colors group-hover:bg-line-strong group-focus-visible:bg-clay group-active:bg-clay" />
        </div>
      </div>
      {children}
    </div>
  )
}
