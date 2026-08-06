import { describe, expect, it } from 'vitest'

import type { RuntimeSkillSummary } from '@/bridge/compat'
import {
  deriveSkillMentionEdit,
  findSkillMentionTarget,
  rankSkillCandidates,
  reconcileSkillMentionBindings,
  selectSkillMention,
  skillMentionSegments,
  skillInputsFromBindings,
  type SkillMentionBinding,
} from './skillMentions'

const commit: RuntimeSkillSummary = {
  source: 'agents',
  name: 'commit',
  description: 'Create a commit from the current changes.',
  path: '/Users/me/.agents/skills/commit/SKILL.md',
  disabled: false,
}

const review: RuntimeSkillSummary = {
  source: 'agents',
  name: 'review-pr',
  description: 'Review a pull request.',
  path: '/Users/me/.agents/skills/review-pr/SKILL.md',
  disabled: false,
}

describe('skill mention targeting', () => {
  it('opens only for a lowercase token at a line or whitespace boundary', () => {
    expect(findSkillMentionTarget('$com', 4)).toEqual({ start: 0, end: 4, query: 'com' })
    expect(findSkillMentionTarget('please $rev now', 11)).toEqual({ start: 7, end: 11, query: 'rev' })
    expect(findSkillMentionTarget('$HOME', 5)).toBeNull()
    expect(findSkillMentionTarget('echo \\$commit', 13)).toBeNull()
    expect(findSkillMentionTarget('word$commit', 11)).toBeNull()
  })

  it('ranks enabled candidates deterministically', () => {
    const disabled = { ...commit, name: 'compare', disabled: true }
    expect(rankSkillCandidates([review, disabled, commit], 'com').map((skill) => skill.name))
      .toEqual(['commit'])
    expect(rankSkillCandidates([review, commit], '').map((skill) => skill.name))
      .toEqual(['commit', 'review-pr'])
  })
})

describe('skill mention bindings', () => {
  it('selects a visible token while preserving an exact hidden path', () => {
    const selected = selectSkillMention('Use $com now', { start: 4, end: 8, query: 'com' }, commit)

    expect(selected.value).toBe('Use $commit now')
    expect(selected.binding).toEqual({
      start: 4,
      end: 11,
      name: 'commit',
      path: commit.path,
    })
  })

  it('moves bindings around external edits and removes an edited token', () => {
    const binding: SkillMentionBinding = { start: 4, end: 11, name: 'commit', path: commit.path }

    expect(reconcileSkillMentionBindings(
      'Use $commit',
      'Please Use $commit',
      [binding],
      { start: 0, end: 0 },
    ))
      .toEqual([{ ...binding, start: 11, end: 18 }])
    expect(reconcileSkillMentionBindings(
      'Use $commit',
      'Use $commits',
      [binding],
      { start: 11, end: 11 },
    ))
      .toEqual([binding])
    expect(reconcileSkillMentionBindings(
      'Use $commit',
      'Use $comit',
      [binding],
      { start: 8, end: 9 },
    ))
      .toEqual([])
  })

  it('does not transfer a binding to an identical handwritten token', () => {
    const value = '$commit $commit'
    const binding: SkillMentionBinding = { start: 0, end: 7, name: 'commit', path: commit.path }

    expect(reconcileSkillMentionBindings(value, '$commit', [binding], { start: 0, end: 8 }))
      .toEqual([])
    expect(reconcileSkillMentionBindings(value, '$commit', [binding]))
      .toEqual([])
  })

  it('derives insert, deletion, paste, and IME ranges from value and caret state', () => {
    expect(deriveSkillMentionEdit('ab', 'acb', { start: 1, end: 1 }, 2))
      .toEqual({ start: 1, end: 1 })
    expect(deriveSkillMentionEdit('$commit $commit', '$commit', { start: 0, end: 8 }, 0))
      .toEqual({ start: 0, end: 8 })
    expect(deriveSkillMentionEdit('$commit', '$commit then continue', { start: 7, end: 7 }, 21))
      .toEqual({ start: 7, end: 7 })
    expect(deriveSkillMentionEdit('Use provisional now', 'Use 提交 now', { start: 4, end: 15 }, 6))
      .toEqual({ start: 4, end: 15 })
    expect(deriveSkillMentionEdit('abc', 'ac', { start: 1, end: 1 }, null)).toBeNull()
  })

  it('keeps a selected skill bound while ordinary text is appended', () => {
    const binding: SkillMentionBinding = { start: 4, end: 11, name: 'commit', path: commit.path }
    const previousValue = 'Use $commit'
    const nextValue = 'Use $commit to finish'

    expect(reconcileSkillMentionBindings(
      previousValue,
      nextValue,
      [binding],
      deriveSkillMentionEdit(
        previousValue,
        nextValue,
        { start: previousValue.length, end: previousValue.length },
        nextValue.length,
      ),
    )).toEqual([binding])
  })

  it('does not transfer a binding through a shared-prefix selection replacement', () => {
    const previousValue = '$commit foo $commit'
    const nextValue = '$commit bar $commit'
    const bindings: SkillMentionBinding[] = [
      { start: 0, end: 7, name: 'commit', path: commit.path },
      { start: 12, end: 19, name: 'commit', path: commit.path },
    ]
    const edit = deriveSkillMentionEdit(
      previousValue,
      nextValue,
      { start: 0, end: 11 },
      11,
    )

    expect(edit).toEqual({ start: 0, end: 11 })
    expect(reconcileSkillMentionBindings(previousValue, nextValue, bindings, edit))
      .toEqual([bindings[1]])
  })

  it('preserves an earlier binding across successive IME composition replacements', () => {
    const binding: SkillMentionBinding = { start: 4, end: 11, name: 'commit', path: commit.path }
    const initialValue = 'Use $commit '
    const provisionalValue = 'Use $commit 临'
    const committedValue = 'Use $commit 提交'
    const firstEdit = deriveSkillMentionEdit(
      initialValue,
      provisionalValue,
      { start: 12, end: 12 },
      13,
    )
    const secondEdit = deriveSkillMentionEdit(
      provisionalValue,
      committedValue,
      { start: 12, end: 13 },
      14,
    )

    expect(reconcileSkillMentionBindings(initialValue, provisionalValue, [binding], firstEdit))
      .toEqual([binding])
    expect(reconcileSkillMentionBindings(provisionalValue, committedValue, [binding], secondEdit))
      .toEqual([binding])
  })

  it('deduplicates submitted paths and exposes bound segments for rendering', () => {
    const value = '$commit then $commit'
    const bindings: SkillMentionBinding[] = [
      { start: 0, end: 7, name: 'commit', path: commit.path },
      { start: 13, end: 20, name: 'commit', path: commit.path },
    ]

    expect(skillInputsFromBindings(value, bindings)).toEqual([
      { type: 'skill', name: 'commit', path: commit.path },
    ])
    expect(skillMentionSegments(value, bindings).map((segment) => ({
      text: segment.text,
      bound: Boolean(segment.binding),
    }))).toEqual([
      { text: '$commit', bound: true },
      { text: ' then ', bound: false },
      { text: '$commit', bound: true },
    ])
  })
})
