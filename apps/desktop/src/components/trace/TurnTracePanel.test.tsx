import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import type { TraceSpan } from '../../type/trace'
import {
  DEFAULT_TRACE_VIEW_MODE,
  TraceSpanNavigator,
} from './TurnTracePanel'
import type { FlatTraceTreeNode } from './traceViewModel'

const root: TraceSpan = {
  traceId: 'turn-1',
  spanId: 'turn-1',
  parentSpanId: null,
  spanKind: 'turn',
  spanName: 'turn.run',
  status: 'succeeded',
  sessionId: 'session-1',
  turnId: 'turn-1',
  stepId: null,
  toolRunId: null,
  startedAt: 1_000,
  endedAt: 5_000,
  durationMs: 4_000,
  attributes: {},
  errorType: null,
  errorCode: null,
  errorMessage: null,
}

const tool: TraceSpan = {
  ...root,
  spanId: 'tool-1',
  parentSpanId: 'step-1',
  spanKind: 'tool_run',
  spanName: 'tool.run',
  toolRunId: 'tool-1',
  startedAt: 2_000,
  endedAt: 3_000,
  durationMs: 1_000,
  attributes: { toolName: 'bash' },
}

const transport: TraceSpan = {
  ...root,
  spanId: 'transport-1',
  parentSpanId: 'step-1',
  spanKind: 'transport_attempt',
  spanName: 'model.transport',
  stepId: 'step-1',
  startedAt: 1_600,
  endedAt: 1_800,
  durationMs: 200,
  attributes: { transportAttempt: 1 },
}

const step: TraceSpan = {
  ...root,
  spanId: 'step-1',
  parentSpanId: 'turn-1',
  spanKind: 'step',
  spanName: 'agent.step',
  stepId: 'step-1',
  startedAt: 1_500,
  endedAt: 4_500,
  durationMs: 3_000,
}

const transportNode: FlatTraceTreeNode = { span: transport, children: [], depth: 2 }
const toolNode: FlatTraceTreeNode = { span: tool, children: [], depth: 2 }
const stepNode: FlatTraceTreeNode = {
  span: step,
  children: [transportNode, toolNode],
  depth: 1,
}
const rows: FlatTraceTreeNode[] = [
  { span: root, children: [stepNode], depth: 0 },
  stepNode,
  transportNode,
  toolNode,
]

describe('TraceSpanNavigator', () => {
  it('defaults the trace panel to the call tree design', () => {
    expect(DEFAULT_TRACE_VIEW_MODE).toBe('tree')
  })

  it('renders a tree with inline durations and no duration column', () => {
    const markup = renderToStaticMarkup(
      <TraceSpanNavigator
        rows={rows}
        viewMode="tree"
        selectedSpanId="turn-1"
        turnStartedAt={1_000}
        turnDurationMs={4_000}
        waterfallZoom={1}
        collapsedSpanIds={new Set()}
        onViewModeChange={vi.fn()}
        onWaterfallZoomChange={vi.fn()}
        onToggleCollapse={vi.fn()}
        onSelect={vi.fn()}
      />,
    )

    expect(markup).toContain('data-trace-view-mode="tree"')
    expect(markup).toContain('data-trace-view-tab="tree"')
    expect(markup).toContain('data-trace-inline-duration="true"')
    expect(markup).toContain('data-trace-collapse-toggle="turn-1"')
    expect(markup).toContain('aria-expanded="true"')
    expect(markup).toContain('lucide-workflow')
    expect(markup).toContain('lucide-network')
    expect(markup).toContain('bg-status-success')
    expect(markup).not.toContain('bg-emerald-500')
    expect(markup).toContain('data-trace-span-row="tool-1"')
    expect(markup).not.toContain('data-trace-duration-column')
    expect(markup).not.toContain('data-trace-waterfall-axis')
  })

  it('collapses descendants without selecting or removing the parent row', () => {
    const markup = renderToStaticMarkup(
      <TraceSpanNavigator
        rows={rows}
        viewMode="tree"
        selectedSpanId="tool-1"
        turnStartedAt={1_000}
        turnDurationMs={4_000}
        waterfallZoom={1}
        collapsedSpanIds={new Set(['turn-1'])}
        onViewModeChange={vi.fn()}
        onWaterfallZoomChange={vi.fn()}
        onToggleCollapse={vi.fn()}
        onSelect={vi.fn()}
      />,
    )

    expect(markup).toContain('data-trace-collapse-toggle="turn-1"')
    expect(markup).toContain('aria-expanded="false"')
    expect(markup).toContain('data-trace-span-row="turn-1"')
    expect(markup).not.toContain('data-trace-span-row="tool-1"')
  })

  it('renders Waterfall as a separate view on the shared turn clock', () => {
    const markup = renderToStaticMarkup(
      <TraceSpanNavigator
        rows={rows}
        viewMode="waterfall"
        selectedSpanId="tool-1"
        turnStartedAt={1_000}
        turnDurationMs={4_000}
        waterfallZoom={2}
        collapsedSpanIds={new Set(['turn-1'])}
        onViewModeChange={vi.fn()}
        onWaterfallZoomChange={vi.fn()}
        onToggleCollapse={vi.fn()}
        onSelect={vi.fn()}
      />,
    )

    expect(markup).toContain('data-trace-view-mode="waterfall"')
    expect(markup).toContain('data-trace-view-tab="waterfall"')
    expect(markup).toContain('data-waterfall-scroll-region="true"')
    expect(markup).toContain('data-waterfall-canvas="true"')
    expect(markup).toContain('width:200%')
    expect(markup).toContain('data-trace-waterfall-axis="true"')
    expect(markup).toContain('data-trace-waterfall-row="tool-1"')
    expect(markup).toContain('data-waterfall-bar="tool-1"')
    expect(markup).toContain('lucide-workflow')
    expect(markup).toContain('lucide-network')
    expect(markup).toContain('bg-trace-bar-turn')
    expect(markup).toContain('bg-trace-bar-step')
    expect(markup).toContain('bg-trace-bar-transport')
    expect(markup).toContain('bg-trace-bar-tool')
    expect(markup).toContain('text-trace-bar-ink')
    expect(markup).not.toContain('bg-sky-500')
    expect(markup).not.toContain('bg-blue-500')
    expect(markup).not.toContain('bg-amber-500')
    expect(markup).not.toContain('bg-cyan-600')
    expect(markup).toContain('data-waterfall-tick="1000"')
    expect(markup).toContain('data-waterfall-zoom-control="out"')
    expect(markup).toContain('data-waterfall-zoom-control="in"')
    expect(markup).toContain('data-waterfall-zoom-control="reset"')
    expect(markup).toContain('left:25%')
    expect(markup).toContain('width:25%')
    expect(markup).toContain('工具：bash')
    expect(markup).not.toContain('grid-cols-[minmax(118px,42%)')
    expect(markup).not.toContain('data-trace-duration-column')
  })
})
