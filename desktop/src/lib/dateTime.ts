const beijingFormatter = new Intl.DateTimeFormat('en-CA', {
  timeZone: 'Asia/Shanghai',
  year: 'numeric',
  month: '2-digit',
  day: '2-digit',
  hour: '2-digit',
  minute: '2-digit',
  second: '2-digit',
  hourCycle: 'h23',
})

export function formatBeijingDateTime(value: string): string {
  const timestamp = Date.parse(value)
  if (!Number.isFinite(timestamp)) return '—'

  const parts = new Map(
    beijingFormatter
      .formatToParts(timestamp)
      .map((part) => [part.type, part.value]),
  )
  return `${parts.get('year')}-${parts.get('month')}-${parts.get('day')} ${parts.get('hour')}:${parts.get('minute')}:${parts.get('second')} (Asia/Shanghai)`
}

/** `HH:mm`，东八区；时间无法解析时返回占位符。 */
export function formatBeijingClock(value: string): string {
  const timestamp = Date.parse(value)
  if (!Number.isFinite(timestamp)) return '—'
  const parts = new Map(beijingFormatter.formatToParts(timestamp).map((part) => [part.type, part.value]))
  return `${parts.get('hour')}:${parts.get('minute')}`
}

/** 列表里的时间：今天显示钟点，昨天显示“昨天”，更早显示月日（东八区）。 */
export type BeijingDayStamp =
  | { kind: 'today', clock: string }
  | { kind: 'yesterday' }
  | { kind: 'date', month: number, day: number }

function beijingDate(timestamp: number): { year: number, month: number, day: number } {
  const parts = new Map(beijingFormatter.formatToParts(timestamp).map((part) => [part.type, part.value]))
  return { year: Number(parts.get('year')), month: Number(parts.get('month')), day: Number(parts.get('day')) }
}

/** 比较两个时刻在东八区日历上相差几天；`value` 无法解析时返回 `null`。 */
export function beijingDayStamp(value: string, now: number): BeijingDayStamp | null {
  const timestamp = Date.parse(value)
  if (!Number.isFinite(timestamp)) return null
  const date = beijingDate(timestamp)
  const today = beijingDate(now)
  const days = Math.round(
    (Date.UTC(today.year, today.month - 1, today.day) - Date.UTC(date.year, date.month - 1, date.day)) / 86_400_000,
  )
  if (days <= 0) return { kind: 'today', clock: formatBeijingClock(value) }
  if (days === 1) return { kind: 'yesterday' }
  return { kind: 'date', month: date.month, day: date.day }
}

/** 从 `startedAt` 到 `now` 的整秒数，不小于 0；时间无法解析时为 0。 */
export function elapsedSeconds(startedAt: string, now: number): number {
  const started = Date.parse(startedAt)
  if (!Number.isFinite(started)) return 0
  return Math.max(0, Math.floor((now - started) / 1000))
}
