import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'

import type { RuntimeSkillDetail, RuntimeSkillSummary } from '@/bridge/compat'
import { SkillDetailView, displaySkillName } from './SkillDetail'

const skill: RuntimeSkillSummary = {
  source: 'agents',
  name: 'codebase-design',
  description: 'Vocabulary for deep-module design',
  path: '/Users/me/.agents/skills/codebase-design/SKILL.md',
  disabled: false,
}

const detail: RuntimeSkillDetail = {
  ...skill,
  body: 'Design **deep modules**: a lot of behaviour behind a small interface.\n\n## Glossary\n\nModule — anything with an interface.',
}

describe('displaySkillName', () => {
  it('turns the directory name into a title', () => {
    expect(displaySkillName('codebase-design')).toBe('Codebase Design')
    expect(displaySkillName('commit')).toBe('Commit')
  })
})

describe('SkillDetailView', () => {
  it('renders the title, badge, description, path, and markdown body', () => {
    const markup = renderToStaticMarkup(
      <SkillDetailView skill={skill} detail={detail} loading={false} error={null} onBack={vi.fn()} />,
    )

    expect(markup).toContain('Codebase Design')
    expect(markup).toContain('Skill')
    expect(markup).toContain('Vocabulary for deep-module design')
    expect(markup).toContain('~/.agents/skills/codebase-design/SKILL.md')
    expect(markup).toContain('<strong>deep modules</strong>')
    expect(markup).toContain('Glossary')
    expect(markup).toContain('返回列表')
  })

  it('shows the loading and failure states of the body card', () => {
    const loading = renderToStaticMarkup(
      <SkillDetailView skill={skill} detail={null} loading error={null} onBack={vi.fn()} />,
    )
    expect(loading).toContain('正在读取 Skill 正文……')

    const failed = renderToStaticMarkup(
      <SkillDetailView
        skill={skill}
        detail={null}
        loading={false}
        error="path is not a SKILL.md directly inside a configured skill root"
        onBack={vi.fn()}
      />,
    )
    expect(failed).toContain('role="alert"')
    expect(failed).toContain('读取 Skill 详情失败：path is not a SKILL.md')
  })
})
