import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'

import type { RuntimePlanStep } from '@/bridge/compat'
import type { TurnPlanView } from '@/types/chat'
import { PlanCard } from './PlanCard'

const steps: RuntimePlanStep[] = [
  { step: 'read schema', status: 'completed' },
  { step: 'add migration', status: 'in_progress' },
  { step: 'wire runner', status: 'pending' },
  { step: 'ship it', status: 'pending' },
]

function plan(overrides: Partial<TurnPlanView> = {}): TurnPlanView {
  return {
    explanation: null,
    steps,
    updateCount: 3,
    startedAt: '2026-08-09T00:00:00Z',
    updatedAt: '2026-08-09T00:08:12Z',
    ...overrides,
  }
}

describe('PlanCard', () => {
  it('shows completed over total', () => {
    const markup = renderToStaticMarkup(<PlanCard plan={plan()} />)

    expect(markup).toContain('1 / 4')
    expect(markup).toContain('已更新 3 次')
  })

  it('keeps the server order', () => {
    const markup = renderToStaticMarkup(<PlanCard plan={plan()} />)

    const positions = steps.map((step) => markup.indexOf(step.step))
    expect(positions).toEqual([...positions].sort((left, right) => left - right))
    expect(positions.every((position) => position >= 0)).toBe(true)
  })

  it('gives every status a text label, not just a colour', () => {
    const markup = renderToStaticMarkup(<PlanCard plan={plan()} />)

    expect(markup).toContain('已完成')
    expect(markup).toContain('进行中')
    expect(markup).toContain('待办')
  })

  it('renders the explanation only when present', () => {
    const withExplanation = renderToStaticMarkup(
      <PlanCard plan={plan({ explanation: 'scoping the work' })} />,
    )
    expect(withExplanation).toContain('scoping the work')
    expect(withExplanation).toContain('<p class=')

    // 没有 explanation 时不留空占位。（`<p` 会命中图标 SVG 里的 `<path>`，要匹配到属性。）
    const withoutExplanation = renderToStaticMarkup(<PlanCard plan={plan()} />)
    expect(withoutExplanation).not.toContain('<p class=')
  })

  it('renders nothing for an empty plan', () => {
    expect(renderToStaticMarkup(<PlanCard plan={plan({ steps: [] })} />)).toBe('')
  })

  it('does not advance a pending step on its own', () => {
    const allPending: RuntimePlanStep[] = [
      { step: 'first', status: 'pending' },
      { step: 'second', status: 'pending' },
    ]

    const markup = renderToStaticMarkup(<PlanCard plan={plan({ steps: allPending })} />)

    expect(markup).toContain('0 / 2')
    expect(markup).not.toContain('进行中')
  })

  it('automatically collapses a completed plan into its duration summary', () => {
    const completed = steps.map((step) => ({ ...step, status: 'completed' as const }))
    const markup = renderToStaticMarkup(<PlanCard plan={plan({ steps: completed })} />)

    expect(markup).toContain('计划完成 · 4 步 · 共 8 分 12 秒')
    expect(markup).toContain('aria-expanded="false"')
    expect(markup).not.toContain('read schema')
  })

  it('collapses completed rows when a long plan has more than twelve steps', () => {
    const longPlan = Array.from({ length: 13 }, (_, index): RuntimePlanStep => ({
      step: `step ${index + 1}`,
      status: index < 8 ? 'completed' : index === 8 ? 'in_progress' : 'pending',
    }))
    const markup = renderToStaticMarkup(<PlanCard plan={plan({ steps: longPlan })} />)

    expect(markup).toContain('已完成 8 步')
    expect(markup).not.toContain('title="step 1"')
    expect(markup).toContain('step 9')
  })
})
