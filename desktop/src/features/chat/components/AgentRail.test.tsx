import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import type { AgentRailItem } from '../agentRailModel'
import { AgentRail } from './AgentRail'
import { AgentRailCard } from './AgentRailCard'

function item(overrides: Partial<AgentRailItem> = {}): AgentRailItem {
  return {
    sessionId: 'child-1',
    role: 'Analyst',
    task: 'compute_roi',
    status: 'running',
    toolActivity: { name: 'python', attempt: 3 },
    elapsedMs: 108_000,
    tokens: 96_400,
    steps: 8,
    orchestrator: false,
    ...overrides,
  }
}

const items: AgentRailItem[] = [
  item({
    sessionId: 'parent-1',
    role: '主控',
    task: 'Q3 渠道 ROI 异常复盘',
    orchestrator: true,
    tokens: 42_100,
    toolActivity: null,
  }),
  item({ sessionId: 'child-1', role: 'Researcher', task: 'fetch_channel_spend', status: 'completed', tokens: 31_600 }),
  item({ sessionId: 'child-2', role: 'Reviewer', task: 'review', status: 'idle', tokens: 0, elapsedMs: null, toolActivity: null }),
]

describe('AgentRail', () => {
  it('lists every agent with its status, duration, and token total', () => {
    const markup = renderToStaticMarkup(
      <AgentRail items={items} selectedSessionId={null} error={null} onSelect={vi.fn()} onCollapse={vi.fn()} />,
    )

    expect(markup).toContain('data-agent-rail="true"')
    expect(markup).toContain('主控')
    expect(markup).toContain('Researcher')
    expect(markup).toContain('Reviewer')
    expect(markup).toContain('1 分 48 秒')
    expect(markup).toContain('42.1k')
    expect(markup).toContain('31.6k')
    expect(markup).toContain('运行中')
    expect(markup).not.toContain('已完成')
  })

  it('summarises how many agents are running and standing by', () => {
    const markup = renderToStaticMarkup(
      <AgentRail items={items} selectedSessionId={null} error={null} onSelect={vi.fn()} onCollapse={vi.fn()} />,
    )

    expect(markup).toContain('1 运行中')
    expect(markup).toContain('2 待命')
  })

  it('separates the orchestrator from a compact child-agent section', () => {
    const markup = renderToStaticMarkup(
      <AgentRail items={items} selectedSessionId={null} error={null} onSelect={vi.fn()} onCollapse={vi.fn()} />,
    )

    expect(markup).toContain('data-agent-orchestrator="true"')
    expect(markup).toContain('data-agent-children="true"')
    expect(markup).toContain('子智能体 2')
    expect(markup).toContain('fetch_channel_spend')
    expect(markup).toContain('data-agent-token-scale="child-1"')
    expect(markup).toContain('data-agent-token-scale="child-2"')
  })

  it('totals tokens across the tree in its header and never shows a price', () => {
    const markup = renderToStaticMarkup(
      <AgentRail items={items} selectedSessionId={null} error={null} onSelect={vi.fn()} onCollapse={vi.fn()} />,
    )

    expect(markup).toContain('data-agent-rail-total="true"')
    expect(markup).not.toContain('data-agent-rail-footer="true"')
    // 42.1k + 31.6k + 0
    expect(markup).toContain('73.7k')
    expect(markup).not.toContain('¥')
  })

  it('marks the agent that is open in the centre column', () => {
    const markup = renderToStaticMarkup(
      <AgentRail items={items} selectedSessionId="child-1" error={null} onSelect={vi.fn()} onCollapse={vi.fn()} />,
    )

    expect(markup).toContain('aria-current="true"')
    expect(markup).toContain('border-clay')
  })

  it('surfaces a list failure without hiding the agents it already knows about', () => {
    const markup = renderToStaticMarkup(
      <AgentRail items={items} selectedSessionId={null} error="bridge down" onSelect={vi.fn()} onCollapse={vi.fn()} />,
    )

    expect(markup).toContain('role="alert"')
    expect(markup).toContain('bridge down')
    expect(markup).toContain('Researcher')
  })

  it('offers a control to collapse the right rail', () => {
    const markup = renderToStaticMarkup(
      <AgentRail items={items} selectedSessionId={null} error={null} onSelect={vi.fn()} onCollapse={vi.fn()} />,
    )

    expect(markup).toContain('aria-label="收起智能体面板"')
    expect(markup).toContain('aria-expanded="true"')
    expect(markup).toContain('lucide-panel-right-close')
  })
})

describe('AgentRailCard', () => {
  it('shows the child task and token scale without presenting task progress', () => {
    const markup = renderToStaticMarkup(
      <AgentRailCard item={item()} selected={false} onSelect={vi.fn()} />,
    )

    expect(markup).toContain('compute_roi')
    expect(markup).toContain('data-agent-card-meta="true"')
    expect(markup).toContain('运行中')
    expect(markup).toContain('已运行 1 分 48 秒')
    expect(markup).toContain('data-agent-token-scale="child-1"')
    expect(markup).toContain('style="width:100%"')
    expect(markup).not.toContain('python · 第 3 次调用')
    expect(markup).not.toContain('role="progressbar"')
    expect(markup).not.toContain('sr-only')
  })

  it('gives the orchestrator its fixed avatar and sub-agents a graphic avatar', () => {
    const orchestrator = renderToStaticMarkup(
      <AgentRailCard item={item({ role: '主控', orchestrator: true })} selected={false} onSelect={vi.fn()} />,
    )
    const subAgent = renderToStaticMarkup(
      <AgentRailCard item={item()} selected={false} onSelect={vi.fn()} />,
    )
    const sameSubAgent = renderToStaticMarkup(
      <AgentRailCard item={item()} selected={false} onSelect={vi.fn()} />,
    )
    const orchestratorSrc = orchestrator.match(/<img src="([^"]+)"/)?.[1]
    const subAgentSrc = subAgent.match(/<img src="([^"]+)"/)?.[1]
    const sameSubAgentSrc = sameSubAgent.match(/<img src="([^"]+)"/)?.[1]

    expect(orchestrator).toContain('data-agent-avatar="orchestrator"')
    expect(subAgent).toContain('data-agent-avatar="subagent"')
    expect(subAgent).not.toContain('>A<')
    expect(orchestratorSrc).toMatch(/^data:image\/svg\+xml/)
    expect(subAgentSrc).toMatch(/^data:image\/svg\+xml/)
    expect(subAgentSrc).not.toBe(orchestratorSrc)
    expect(sameSubAgentSrc).toBe(subAgentSrc)
  })

  it('keeps an unknown duration visible on the child card as a placeholder', () => {
    const markup = renderToStaticMarkup(
      <AgentRailCard
        item={item({ status: 'idle', toolActivity: null, elapsedMs: null })}
        selected={false}
        onSelect={vi.fn()}
      />,
    )

    expect(markup).not.toContain('animate-pulse')
    expect(markup).toContain('--:--')
    expect(markup).toContain('待命')
    expect(markup).toContain('data-agent-card-meta="true"')
    expect(markup).toContain('data-agent-token-scale="child-1"')
  })

  it('labels a completed child as standby because it can receive a follow-up turn', () => {
    const markup = renderToStaticMarkup(
      <AgentRailCard
        item={item({ status: 'completed' })}
        selected={false}
        onSelect={vi.fn()}
      />,
    )

    expect(markup).toContain('待命')
    expect(markup).not.toContain('已完成')
    expect(markup).toContain('上次运行 1 分 48 秒')
  })
})
