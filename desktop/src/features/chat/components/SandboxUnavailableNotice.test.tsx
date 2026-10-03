import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'

import { SandboxUnavailableNotice } from './SandboxUnavailableNotice'

describe('SandboxUnavailableNotice', () => {
  /** permissions.md §6：沙箱不可用时常驻提示，并能看到自检失败的原因。 */
  it('shows that bash is disabled and the self-check reason', () => {
    const markup = renderToStaticMarkup(
      <SandboxUnavailableNotice
        sandbox={{ state: 'unavailable', reason: 'a write the sandbox forbids succeeded' }}
      />,
    )

    expect(markup).toContain('沙箱不可用，bash 已停用')
    expect(markup).toContain('读、写、编辑、搜索文件照常可用。')
    expect(markup).toContain('<summary')
    expect(markup).toContain('查看原因')
    expect(markup).toContain('启动自检失败：a write the sandbox forbids succeeded')
  })

  it('renders nothing while the sandbox is available or not yet known', () => {
    expect(renderToStaticMarkup(<SandboxUnavailableNotice sandbox={{ state: 'available' }} />)).toBe('')
    expect(renderToStaticMarkup(<SandboxUnavailableNotice sandbox={null} />)).toBe('')
  })
})
