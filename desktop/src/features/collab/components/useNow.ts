import { useEffect, useState } from 'react'

/** 当前时间，每 `intervalMs` 刷新一次；用于“已用时间”“昨天”这类随时间变化的显示。 */
export function useNow(intervalMs: number): number {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), intervalMs)
    return () => window.clearInterval(timer)
  }, [intervalMs])
  return now
}
