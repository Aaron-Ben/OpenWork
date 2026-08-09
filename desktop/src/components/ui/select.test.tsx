import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'

import { Select, SelectTrigger, SelectValue } from './select'

function trigger(label: string, className?: string) {
  return renderToStaticMarkup(
    <Select value="a">
      <SelectTrigger className={className} aria-label="示例">
        <SelectValue>{label}</SelectValue>
      </SelectTrigger>
    </Select>,
  )
}

describe('SelectTrigger', () => {
  /*
    触发器是定宽的（调用点都给了 clamp 或 min-w），值必须单行截断：
    长模型名折行会把 h-8 撑破，从聊天输入框里溢出去。
    截断只能加在触发器上 —— Radix 的 Value 不把 className 转发到它渲染的 span，
    在调用点写 truncate 是静默失效的，所以这条要有测试守着。
  */
  it('clips its value to a single line', () => {
    const markup = trigger('deepseek-v4-flash · Plus')

    expect(markup).toContain('overflow-hidden')
    expect(markup).toContain('[&amp;&gt;span]:truncate')
    expect(markup).toContain('[&amp;&gt;span]:min-w-0')
  })

  it('keeps caller classes last so they can still override the defaults', () => {
    const markup = trigger('全部状态', 'h-10 min-w-40 border border-line px-3')
    const cls = markup.match(/class="([^"]*)"/)?.[1] ?? ''

    expect(cls.indexOf('h-10')).toBeGreaterThan(cls.indexOf('overflow-hidden'))
    expect(cls).toContain('min-w-40')
  })
})
