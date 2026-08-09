import { useEffect, useState } from 'react'

/**
 * 每秒返回一次新的 `Date.now()`，只在 `active` 为真时走表。
 * 耗时是"现在减去开始时刻"，没有这个心跳，运行中的计时会停在最后一次事件的时刻。
 */
export function useNowTicker(active: boolean, intervalMs = 1_000): number {
  const [now, setNow] = useState(() => Date.now())

  useEffect(() => {
    if (!active) return
    setNow(Date.now())
    const timer = setInterval(() => setNow(Date.now()), intervalMs)
    return () => clearInterval(timer)
  }, [active, intervalMs])

  return now
}
