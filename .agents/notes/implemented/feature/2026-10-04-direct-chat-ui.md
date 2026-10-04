# Agent Note: 私聊界面

Status: implemented

## 问题

第 2 步要能演示核心效果：用户新建一个 Agent，给它发消息，看到它的回复与状态。需要决定界面的布局、视觉风格、组件写法，以及怎样显示 Markdown、模型列表、Agent 状态与连接状态。

## 决策

- 两栏布局：左侧是 Agent 列表，每个 Agent 对应一个私聊房间，带状态点；右侧是聊天（`apps/desktop/src/App.tsx`）。cumora 的左侧导航栏（`cumora:src/desktop/Rail.tsx`）在有多个页面时再加。
- 视觉风格：框架取“控制台”方向，侧栏、顶栏、状态与元信息用等宽字体，绿色强调色只用于回复中、选中与主按钮；正文取“纸面”方向，消息不加气泡，名字和时间在上，正文的字号与行距偏大，行宽有上限。浅色与深色两套主题跟随系统切换。颜色与字体是 `apps/desktop/src/index.css` 中的设计变量，只用系统字体。
- 系统的红黄绿按钮放进侧栏顶部（`titleBarStyle: "hiddenInset"`）。窗口只显示界面自己的页面：消息里的 http 与 https 链接交给系统浏览器打开，其余导航一律丢弃（`apps/desktop/electron/navigation.ts`）。
- 组件按 shadcn/ui 的做法写，源码放在 `apps/desktop/src/components/ui/`。
- 消息正文按 Markdown 渲染，不渲染原始 HTML。代码块按围栏上写的语言高亮，不自动猜语言。highlight.js 输出类名，颜色交给 CSS 的设计变量，这一点与 cumora（`cumora:src/components/Message.tsx`）相同。
- 新建 Agent 时填名字、人设与模型。Computer 启动时运行 `opencode models`，把可用模型列表上报给 Server，界面用下拉框显示。
- 聊天底部显示“正在回复”；出错时显示原因，并说明下一条消息到来时会自动重试。失败的 Turn 不推进已读位置，下一条消息到来时自动重试，所以没有单独的重试按钮。
- 侧栏底部显示界面与 Server 的 SSE 连接状态。没有“Computer 是否连接”的显示：主进程等 Computer 连上 Server 后才打开窗口，Computer 退出时整个应用随之退出，所以窗口存在期间它总是已连接。

界面的数据怎样获取与刷新，见 [SSE 只传失效提示与 Agent 唤醒](../architecture/2026-10-04-sse-invalidation-and-wake.md)。

## 考虑过的方案

**模型名用文本框填写。** 简单，但容易填错。没有采用：Computer 运行 `opencode models` 就能拿到可用列表。

**代码高亮用 shiki。** raft 这样做（`raft:packages/web/src/components/markdown/shikiHighlighter.ts`）。没有采用：它异步加载，配色来自自带主题，不能交给 CSS 的设计变量统一浅色与深色。

## 后果

- 第 2 步就能演示：新建 Agent、发消息、看到回复与状态。
- 界面没有导航栏，看板等页面出现时要加。
- 改了界面后，用 `pnpm preview:shot` 截浅色与深色两张图自查，见 [testing.md](../../../../docs/testing.md)。
