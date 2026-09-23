# 前端规范

适用于 `desktop/src/`（React 19 + TypeScript + zustand + Tailwind v4 + i18next）。架构边界以 [docs/desktop.md](../../docs/desktop.md) 为准。

## 1. 目录

| 位置 | 放什么 |
|---|---|
| `bridge/` | 唯一调用 Tauri `invoke` / 订阅事件的地方；Rust 契约类型（`compat.ts`） |
| `features/<领域>/` | 一个领域的 store、reducer、视图模型、hooks；组件放在其下的 `components/` |
| `components/ui/` | 与业务无关的基础组件（button、select、input） |
| `lib/` | 跨领域的纯工具：`dateTime.ts`、`commandError.ts` |
| `i18n/locales/` | `zh-CN`、`zh-TW`、`en-US` 三份文案 |

- 组件和 store 不直接 `invoke`，通过 `bridge/` 的函数调用 Rust。
- 跨领域导入走 `@/` 别名；同一领域内用相对路径。

## 2. 状态

- 状态分三层，互不污染（desktop.md §4）：Core 推来的服务端状态、运行中 Turn 的实时视图、纯界面状态。
- 服务端状态以 Core 推送为准，前端不自行推断 Turn 是否结束、不按 Trace 判断状态。
- store 用 zustand；状态转换写成纯函数 reducer（`runtimeReducer.ts` 的写法），store 只负责调用它。
- 更新不可变：返回新对象，不修改原有 state。

```ts
// ✅ 正确：reducer 是纯函数，可以单独测试
const next = reduceSessionUpdate(view, envelope)
set({ bySession: { ...state.bySession, [sessionId]: next } })

// ❌ 错误：就地修改，订阅方收不到变化
state.bySession[sessionId].messages.push(message)
```

## 3. 组件

- 组件文件不超过 **400 行**，超过就把逻辑移到同目录的 `.ts` 视图模型或 hook 中，或拆子组件。存量超限的 `TurnTraceDrawer.tsx`、`ReadonlyToolActivity.tsx` 等，新增内容不得让它们继续变大。
- 组件只负责渲染与交互；数据整理、排序、格式化放在纯函数模块里并单独测试（`transcript.ts`、`agentPresentation.ts`）。
- 基础交互组件优先复用 `components/ui/` 与 Radix，不重写下拉、选择器、对话框。

## 4. 文案与格式

- **界面文案一律走 i18n**：`const { t } = useTranslation()`，三种语言同时加键，`i18n.test.ts` 会检查键结构一致。
- 专有名词（OpenCode、Seatbelt、模型名）可以不翻译。
- 时间只用 `lib/dateTime.ts` 显示，禁止对时间字符串做切片或正则（[database.md](database.md) §1.5）。
- 命令错误统一用 `resolveCommandError` 转成 `CommandError`，按 `code` 分支，不解析 `message` 文本。

```tsx
// ✅ 正确
<Button>{t('sidebar.newSession')}</Button>

// ❌ 错误：硬编码文案
<Button>创建会话</Button>
```

## 5. TypeScript

- 不用 `any`；外部数据先当 `unknown`，经类型守卫后再使用（`isCommandError` 的写法）。
- 不用非空断言 `!` 掩盖可能为空的值；处理空值分支。
- 联合类型用 `switch` 穷举，在 `default` 里用 `never` 检查，新增变体时编译报错。

```ts
type LoadState = 'idle' | 'loading' | 'ready' | 'failed'

function loadLabel(state: LoadState): string {
  switch (state) {
    case 'idle': return ''
    case 'loading': return t('common.loading')
    case 'ready': return ''
    case 'failed': return t('common.loadFailed')
    default: {
      const unreachable: never = state
      return unreachable
    }
  }
}
```

## 6. 样式

- 用 Tailwind 类与项目的设计令牌（`text-ink`、`bg-paper` 等）；全局样式只在 `app/theme/globals.css`，不新增 CSS 文件。
- 内联 `style` 只用于运行时计算出的值（进度条宽度这类百分比），静态样式一律用类名。
- 条件类名用 `cn()`（`clsx` + `tailwind-merge`），不手动拼接字符串。

## 违规模式检测

发现以下情况应立即指出并给出修复建议：

- `bridge/` 以外调用 `invoke` 或直接订阅 Tauri 事件
- 就地修改 store 状态；状态转换逻辑写在组件或 store 内而不是纯函数中
- 前端自行推断 Turn 状态或按 Trace 判断状态
- 硬编码界面文案；只加了一种语言的文案
- 对时间字符串做 `slice` / `split` / 正则；解析 `CommandError.message` 做分支
- `any`、没有理由的非空断言、没有穷举检查的联合类型分支
- 组件超过 400 行，或让存量超限组件继续变大
- 静态样式写成内联 `style`；手动拼接类名；新增 CSS 文件
