import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import type { RuntimeSkillDiscovery } from '@/bridge/compat'
import { SkillListView } from './SkillList'

describe('SkillList', () => {
  it('shows path-free skill rows and keeps concrete load failures', () => {
    const discovery: RuntimeSkillDiscovery = {
      skills: [
        {
          source: 'agents',
          name: 'commit',
          description: '创建符合规范的提交。',
          path: '/Users/me/.agents/skills/commit/SKILL.md',
          disabled: true,
        },
      ],
      warnings: [
        {
          path: '/Users/me/.agents/skills/broken/SKILL.md',
          reason: 'skill frontmatter is missing the required description field',
        },
      ],
    }

    const markup = renderToStaticMarkup(
      <SkillListView
        discovery={discovery}
        loading={false}
        error={null}
        onRefresh={vi.fn()}
      />,
    )

    expect(markup).toContain('commit')
    expect(markup).toContain('创建符合规范的提交。')
    expect(markup).not.toContain('/Users/me/.agents/skills/commit/SKILL.md')
    expect(markup).not.toContain('~/.agents/skills/commit/SKILL.md')
    expect(markup).toContain('~/.agents/skills/broken/SKILL.md')
    expect(markup).toContain('title="/Users/me/.agents/skills/broken/SKILL.md"')
    expect(markup).toContain('未加载：skill frontmatter is missing the required description field')
    expect(markup).toContain('刷新')
    expect(markup).toContain('role="switch"')
    expect(markup).toContain('aria-checked="false"')
    expect(markup).toContain('已禁用')
    expect(markup).toContain('不会向模型展示')
  })

  it('renders clickable rows when a selection handler is provided', () => {
    const discovery: RuntimeSkillDiscovery = {
      skills: [
        {
          source: 'agents',
          name: 'commit',
          description: '创建符合规范的提交。',
          path: '/Users/me/.agents/skills/commit/SKILL.md',
          disabled: false,
        },
      ],
      warnings: [],
    }

    const markup = renderToStaticMarkup(
      <SkillListView
        discovery={discovery}
        loading={false}
        error={null}
        onRefresh={vi.fn()}
        onSelectSkill={vi.fn()}
        onSetDisabled={vi.fn()}
      />,
    )

    expect(markup).toContain('<button')
    expect(markup).not.toContain('disabled=""')
    expect(markup).toContain('aria-checked="true"')
  })

  it('disables refresh and toggles while a skill status mutation is pending', () => {
    const discovery: RuntimeSkillDiscovery = {
      skills: [{
        source: 'agents',
        name: 'commit',
        description: '创建符合规范的提交。',
        path: '/Users/me/.agents/skills/commit/SKILL.md',
        disabled: false,
      }],
      warnings: [],
    }
    const markup = renderToStaticMarkup(
      <SkillListView
        discovery={discovery}
        loading={false}
        error={null}
        onRefresh={vi.fn()}
        onSelectSkill={vi.fn()}
        onSetDisabled={vi.fn()}
        updatingNames={new Set(['commit'])}
      />,
    )

    expect(markup.match(/disabled=""/g)).toHaveLength(2)
  })
})
