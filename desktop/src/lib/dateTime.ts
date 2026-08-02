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
