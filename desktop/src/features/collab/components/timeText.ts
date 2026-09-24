/** “多久前”的文案键：一分钟内为“刚刚”，其后依次用分钟、小时、天。 */
export function agoText(seconds: number): { key: `collab.ago.${string}`, values: Record<string, number> } {
  if (seconds < 60) return { key: 'collab.ago.justNow', values: {} }
  if (seconds < 3_600) return { key: 'collab.ago.minutes', values: { count: Math.floor(seconds / 60) } }
  if (seconds < 86_400) return { key: 'collab.ago.hours', values: { count: Math.floor(seconds / 3_600) } }
  return { key: 'collab.ago.days', values: { count: Math.floor(seconds / 86_400) } }
}
