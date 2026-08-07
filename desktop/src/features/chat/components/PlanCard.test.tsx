import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'

import type { RuntimePlanStep } from '@/bridge/compat'
import { PlanCard } from './PlanCard'

const steps: RuntimePlanStep[] = [
  { step: 'read schema', status: 'completed' },
  { step: 'add migration', status: 'in_progress' },
  { step: 'wire runner', status: 'pending' },
  { step: 'ship it', status: 'pending' },
]

describe('PlanCard', () => {
  it('shows completed over total', () => {
    const markup = renderToStaticMarkup(<PlanCard explanation={null} steps={steps} />)

    expect(markup).toContain('1 / 4')
  })

  it('keeps the server order', () => {
    const markup = renderToStaticMarkup(<PlanCard explanation={null} steps={steps} />)

    const positions = steps.map((step) => markup.indexOf(step.step))
    expect(positions).toEqual([...positions].sort((left, right) => left - right))
    expect(positions.every((position) => position >= 0)).toBe(true)
  })

  it('gives every status a text label, not just a colour', () => {
    const markup = renderToStaticMarkup(<PlanCard explanation={null} steps={steps} />)

    expect(markup).toContain('已完成')
    expect(markup).toContain('进行中')
    expect(markup).toContain('待办')
  })

  it('renders the explanation only when present', () => {
    const withExplanation = renderToStaticMarkup(
      <PlanCard explanation="scoping the work" steps={steps} />,
    )
    expect(withExplanation).toContain('scoping the work')
    expect(withExplanation).toContain('<p class=')

    // 没有 explanation 时不留空占位。（`<p` 会命中图标 SVG 里的 `<path>`，要匹配到属性。）
    const withoutExplanation = renderToStaticMarkup(<PlanCard explanation={null} steps={steps} />)
    expect(withoutExplanation).not.toContain('<p class=')
  })

  it('renders nothing for an empty plan', () => {
    expect(renderToStaticMarkup(<PlanCard explanation={null} steps={[]} />)).toBe('')
  })

  it('does not advance a pending step on its own', () => {
    const allPending: RuntimePlanStep[] = [
      { step: 'first', status: 'pending' },
      { step: 'second', status: 'pending' },
    ]

    const markup = renderToStaticMarkup(<PlanCard explanation={null} steps={allPending} />)

    expect(markup).toContain('0 / 2')
    expect(markup).not.toContain('进行中')
  })
})
