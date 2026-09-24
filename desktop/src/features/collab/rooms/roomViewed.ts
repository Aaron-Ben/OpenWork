import type { CollabMessage } from '@/bridge/collab'

/**
 * 用户看到了哪条消息（collaboration.md §8.3、collaboration-desktop.md §7.2）：窗口在前台、
 * 房间里有比上次上报更新的消息时，返回要上报的 sequence；否则返回 `null`。
 */
export function nextViewedSequence(
  messages: CollabMessage[],
  lastReported: number,
  foreground: boolean,
): number | null {
  if (!foreground || messages.length === 0) return null
  const newest = messages[messages.length - 1].sequence
  return newest > lastReported ? newest : null
}
