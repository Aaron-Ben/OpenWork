import type { RuntimeSkillInput, RuntimeSkillSummary } from '@/bridge/compat'

export interface SkillMentionBinding {
  name: string
  path: string
  start: number
  end: number
}

export interface SkillMentionTarget {
  start: number
  end: number
  query: string
}

export interface SkillMentionSegment {
  text: string
  binding?: SkillMentionBinding
}

export interface SkillMentionEdit {
  start: number
  end: number
}

const SKILL_QUERY_RE = /^[a-z0-9-]*$/
const SKILL_QUERY_CHARACTER_RE = /[a-z0-9-]/

export function findSkillMentionTarget(value: string, caret: number): SkillMentionTarget | null {
  const position = Math.max(0, Math.min(caret, value.length))
  let start = position
  while (start > 0 && !/\s/.test(value[start - 1] ?? '')) start -= 1
  if (value[start] !== '$') return null
  if (start > 0 && !/\s/.test(value[start - 1] ?? '')) return null

  const query = value.slice(start + 1, position)
  if (!SKILL_QUERY_RE.test(query)) return null

  let end = position
  while (end < value.length && SKILL_QUERY_CHARACTER_RE.test(value[end] ?? '')) end += 1
  if (!SKILL_QUERY_RE.test(value.slice(start + 1, end))) return null
  return { start, end, query }
}

export function rankSkillCandidates(
  skills: readonly RuntimeSkillSummary[],
  query: string,
): RuntimeSkillSummary[] {
  return skills
    .filter((skill) => !skill.disabled)
    .flatMap((skill) => {
      const score = fuzzyScore(query, skill)
      return score === null ? [] : [{ skill, score }]
    })
    .sort((left, right) => right.score - left.score || compareNames(left.skill.name, right.skill.name))
    .map(({ skill }) => skill)
}

function fuzzyScore(query: string, skill: RuntimeSkillSummary): number | null {
  if (!query) return 0
  const name = skill.name.toLowerCase()
  const normalizedQuery = query.toLowerCase()
  if (name === normalizedQuery) return 1_000
  if (name.startsWith(normalizedQuery)) return 800 - (name.length - normalizedQuery.length)
  const containedAt = name.indexOf(normalizedQuery)
  if (containedAt >= 0) return 600 - containedAt

  const subsequence = subsequenceGap(normalizedQuery, name)
  if (subsequence !== null) return 400 - subsequence

  const descriptionAt = skill.description.toLowerCase().indexOf(normalizedQuery)
  return descriptionAt >= 0 ? 200 - Math.min(descriptionAt, 199) : null
}

function subsequenceGap(query: string, value: string): number | null {
  let valueIndex = 0
  let gap = 0
  for (const character of query) {
    const found = value.indexOf(character, valueIndex)
    if (found < 0) return null
    gap += found - valueIndex
    valueIndex = found + 1
  }
  return gap
}

function compareNames(left: string, right: string): number {
  if (left < right) return -1
  if (left > right) return 1
  return 0
}

export function selectSkillMention(
  value: string,
  target: SkillMentionTarget,
  skill: RuntimeSkillSummary,
): { value: string; binding: SkillMentionBinding; caret: number } {
  const token = `$${skill.name}`
  const nextValue = `${value.slice(0, target.start)}${token}${value.slice(target.end)}`
  const end = target.start + token.length
  return {
    value: nextValue,
    binding: {
      start: target.start,
      end,
      name: skill.name,
      path: skill.path,
    },
    caret: end,
  }
}

export function reconcileSkillMentionBindings(
  previousValue: string,
  nextValue: string,
  bindings: readonly SkillMentionBinding[],
  edit?: SkillMentionEdit | null,
): SkillMentionBinding[] {
  if (previousValue === nextValue) return validSkillMentionBindings(nextValue, bindings)
  if (!edit || edit.start < 0 || edit.end < edit.start || edit.end > previousValue.length) return []

  const insertedLength = nextValue.length - (previousValue.length - (edit.end - edit.start))
  if (insertedLength < 0) return []
  const nextEditEnd = edit.start + insertedLength
  if (
    previousValue.slice(0, edit.start) !== nextValue.slice(0, edit.start)
    || previousValue.slice(edit.end) !== nextValue.slice(nextEditEnd)
  ) return []
  const delta = nextValue.length - previousValue.length

  const mapped = bindings.flatMap((binding) => {
    if (binding.end <= edit.start) return [binding]
    if (binding.start >= edit.end) {
      return [{ ...binding, start: binding.start + delta, end: binding.end + delta }]
    }
    return []
  })
  return validSkillMentionBindings(nextValue, mapped)
}

export function deriveSkillMentionEdit(
  previousValue: string,
  nextValue: string,
  previousSelection: SkillMentionEdit | null,
  nextCaret: number | null,
): SkillMentionEdit | null {
  if (previousValue === nextValue) return { start: 0, end: 0 }
  if (nextCaret === null || nextCaret < 0 || nextCaret > nextValue.length) return null

  if (
    previousSelection
    && previousSelection.start >= 0
    && previousSelection.end >= previousSelection.start
    && previousSelection.end <= previousValue.length
  ) {
    const removedLength = previousSelection.end - previousSelection.start
    const insertedLength = nextValue.length - (previousValue.length - removedLength)
    const nextEditEnd = previousSelection.start + insertedLength
    if (
      insertedLength >= 0
      && nextEditEnd === nextCaret
      && previousValue.slice(0, previousSelection.start) === nextValue.slice(0, previousSelection.start)
      && previousValue.slice(previousSelection.end) === nextValue.slice(nextEditEnd)
    ) return previousSelection
  }

  if (!previousSelection || previousSelection.start !== previousSelection.end) return null
  const removedLength = previousValue.length - nextValue.length
  if (removedLength <= 0) return null

  const position = previousSelection.start
  const edit = nextCaret < position && position - nextCaret === removedLength
    ? { start: nextCaret, end: position }
    : nextCaret === position
      ? { start: position, end: position + removedLength }
      : null
  if (!edit || edit.end > previousValue.length) return null
  return `${previousValue.slice(0, edit.start)}${previousValue.slice(edit.end)}` === nextValue
    ? edit
    : null
}

export function validSkillMentionBindings(
  value: string,
  bindings: readonly SkillMentionBinding[],
): SkillMentionBinding[] {
  return bindings
    .filter((binding) => (
      binding.start >= 0
      && binding.end > binding.start
      && binding.end <= value.length
      && value.slice(binding.start, binding.end) === `$${binding.name}`
    ))
    .sort((left, right) => left.start - right.start || left.end - right.end)
}

export function skillInputsFromBindings(
  value: string,
  bindings: readonly SkillMentionBinding[],
): RuntimeSkillInput[] {
  const seen = new Set<string>()
  return validSkillMentionBindings(value, bindings).flatMap((binding) => {
    if (seen.has(binding.path)) return []
    seen.add(binding.path)
    return [{ type: 'skill', name: binding.name, path: binding.path }]
  })
}

export function skillMentionSegments(
  value: string,
  bindings: readonly SkillMentionBinding[],
): SkillMentionSegment[] {
  const segments: SkillMentionSegment[] = []
  let cursor = 0
  for (const binding of validSkillMentionBindings(value, bindings)) {
    if (binding.start < cursor) continue
    if (binding.start > cursor) segments.push({ text: value.slice(cursor, binding.start) })
    segments.push({ text: value.slice(binding.start, binding.end), binding })
    cursor = binding.end
  }
  if (cursor < value.length) segments.push({ text: value.slice(cursor) })
  return segments
}
