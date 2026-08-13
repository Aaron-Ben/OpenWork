import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import { ChatInput, SkillMentionOverlay } from './ChatInput'

const baseProps = {
  model: 'deepseek-chat',
  modelOptions: [
    {
      modelId: 'deepseek-chat',
      displayName: 'DeepSeek Chat',
      modelTier: 'plus' as const,
      enabled: true,
    },
    {
      modelId: 'deepseek-reasoner',
      displayName: 'DeepSeek Reasoner',
      modelTier: 'pro' as const,
      enabled: true,
    },
  ],
  value: 'Explain this repository',
  isSending: false,
  permissionMode: 'default' as const,
  onValueChange: vi.fn(),
  onModelChange: vi.fn(),
  onPermissionModeChange: vi.fn(),
  onSubmit: vi.fn(),
}

describe('ChatInput toolbar', () => {
  it('renders a selected skill as an inline overlay token without duplicating accessible text', () => {
    const markup = renderToStaticMarkup(
      <SkillMentionOverlay
        value="Use $commit now"
        bindings={[{
          start: 4,
          end: 11,
          name: 'commit',
          path: '/Users/me/.agents/skills/commit/SKILL.md',
        }]}
      />,
    )

    expect(markup).toContain('data-skill-mention-overlay="true"')
    expect(markup).toContain('aria-hidden="true"')
    expect(markup).toContain('data-skill-mention="commit"')
    expect(markup).toContain('bg-clay-soft')
    expect(markup).toContain('Use ')
    expect(markup).toContain('$commit')
    expect(markup).toContain(' now')
  })

  it('renders the compact approval, model, and send controls', () => {
    const markup = renderToStaticMarkup(
      <ChatInput
        {...baseProps}
        contextUsage={{ usedTokens: 66_000, totalTokens: 200_000, estimated: false }}
      />,
    )

    expect(markup).toContain('默认')
    expect(markup).toContain('aria-label="权限模式"')
    expect(markup).toContain('aria-label="选择模型"')
    expect(markup).toContain('DeepSeek Chat · Plus')
    expect(markup).toContain('aria-label="发送"')
    expect(markup).toContain('data-context-usage-ring="true"')
    expect(markup).toContain('data-context-usage-progress="33"')
    expect(markup).not.toContain('data-context-usage-panel')
    expect(markup).toContain('w-auto min-w-[104px] max-w-[min(320px,34vw)]')
    expect(markup).not.toContain('w-[clamp(104px,20vw,200px)]')
    expect(markup).not.toContain('overflow-hidden rounded-[18px]')
    expect(markup).not.toContain('麦克风')
    expect(markup).not.toContain('Demo Provider')
    expect(markup).not.toContain('>运行<')
    expect(markup).toContain('min-h-[80px]')
    expect(markup).toContain('rows="2"')
    expect(markup).not.toContain('min-h-[84px]')
    expect(markup).toContain('min-h-12')
    expect(markup).toContain('size-9')
    expect(markup).not.toContain('min-h-14')
    expect(markup).not.toContain('size-10')
    expect(markup).toContain('data-slot="button"')
    expect(markup).toContain('data-motion-component="chat-input"')
  })

  it('shows the session permission mode selected by Core', () => {
    const markup = renderToStaticMarkup(
      <ChatInput {...baseProps} permissionMode="accept_edits" />,
    )

    expect(markup).toContain('自动接受文件改动')
    expect(markup).toContain('bash 的 mkdir/touch/rm/rmdir/mv/cp/sed 与输出重定向')
    expect(markup).toContain('其他命令仍需审批')
  })

  it('keeps the context affordance available when usage has not been measured', () => {
    const markup = renderToStaticMarkup(
      <ChatInput {...baseProps} contextInspectorOpen onInspectContext={vi.fn()} />,
    )

    expect(markup).toContain('data-context-usage-progress="0"')
    expect(markup).toContain('aria-haspopup="dialog"')
    expect(markup).toContain('aria-expanded="false"')
  })

  it('replaces the send affordance with a compact stop control while streaming', () => {
    const markup = renderToStaticMarkup(<ChatInput {...baseProps} isSending />)

    expect(markup).toContain('aria-label="停止生成"')
    expect(markup).not.toContain('Streaming')
  })

  it('shows the compact command when the input starts with a slash', () => {
    const markup = renderToStaticMarkup(
      <ChatInput {...baseProps} value="/" onSlashCommand={vi.fn()} />,
    )

    expect(markup).toContain('data-slash-command-menu="true"')
    expect(markup).toContain('data-slash-command="compact"')
    expect(markup).toContain('压缩 Conversation')
    expect(markup).toContain('aria-expanded="true"')
  })

  it('shows enabled skill candidates for a dollar trigger', () => {
    const markup = renderToStaticMarkup(
      <ChatInput
        {...baseProps}
        value="$"
        skills={[
          {
            source: 'agents',
            name: 'commit',
            description: 'Create a commit from the current changes.',
            path: '/Users/me/.agents/skills/commit/SKILL.md',
            disabled: false,
          },
          {
            source: 'agents',
            name: 'disabled-skill',
            description: 'Must not be shown.',
            path: '/Users/me/.agents/skills/disabled-skill/SKILL.md',
            disabled: true,
          },
        ]}
      />,
    )

    expect(markup).toContain('data-skill-menu="true"')
    expect(markup).toContain('data-skill-option="commit"')
    expect(markup).toContain('$commit')
    expect(markup).not.toContain('data-skill-option="disabled-skill"')
    expect(markup).not.toContain('data-slash-command-menu="true"')
  })

  it('disables editing and shows progress while compacting', () => {
    const markup = renderToStaticMarkup(<ChatInput {...baseProps} isCompacting />)

    expect(markup).toContain('aria-label="正在压缩 Conversation"')
    expect(markup).toContain('disabled=""')
    expect(markup).not.toContain('aria-label="停止生成"')
  })

  it('renders pending approval content immediately above the input form', () => {
    const markup = renderToStaticMarkup(
      <ChatInput
        {...baseProps}
        topContent={<div data-testid="pending-approval">需要审批</div>}
      />,
    )

    const approvalPosition = markup.indexOf('data-testid="pending-approval"')
    const inputPosition = markup.indexOf('data-motion-component="chat-input"')

    expect(approvalPosition).toBeGreaterThan(-1)
    expect(inputPosition).toBeGreaterThan(approvalPosition)
  })
})

// 输入框是两层叠出来的：透明的 textarea 负责光标与选区，上面的 overlay 负责显示文字。
// 两层的排版一旦不一致，光标会随文字变长越偏越远，选中时还会露出错位的重影。
describe('ChatInput 双层排版必须锁死', () => {
  // 影响字形宽度和基线的属性，任缺一个都会让光标和文字对不上。
  const typography = ['font-serif', 'text-base', 'leading-7', 'px-5', 'py-3']

  function classOf(markup: string, pattern: RegExp): string {
    const match = markup.match(pattern)
    expect(match, `没找到元素：${pattern}`).toBeTruthy()
    return match![1]
  }

  it('overlay 与 textarea 使用同一套字体、字号、行高和内边距', () => {
    const markup = renderToStaticMarkup(<ChatInput {...baseProps} />)

    const overlayClass = classOf(markup, /data-skill-mention-overlay="true"[^>]*class="([^"]*)"/)
    const textareaClass = classOf(markup, /<textarea[^>]*class="([^"]*)"/)

    for (const token of typography) {
      expect(overlayClass, `overlay 缺少 ${token}`).toContain(token)
      expect(textareaClass, `textarea 缺少 ${token}`).toContain(token)
    }
  })

  it('textarea 的文字在选中时也保持透明', () => {
    const markup = renderToStaticMarkup(<ChatInput {...baseProps} />)
    const textareaClass = classOf(markup, /<textarea[^>]*class="([^"]*)"/)

    // globals.css 的 `::selection { color: var(--ink) }` 会把透明文字在选中时画出来，
    // 与 overlay 叠成重影。必须显式覆盖掉，只留选区底色。
    expect(textareaClass).toContain('text-transparent')
    expect(textareaClass).toContain('selection:text-transparent')
    expect(textareaClass).toContain('selection:bg-clay-soft/70')
  })
})
