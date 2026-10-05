# @crew/desktop

Electron 应用：主进程启动并监管 Server 与 Computer，窗口里是 React 界面。界面直接用 HTTP 与 SSE 连接 Server，不经主进程转发；Server 地址与 Desktop 凭证由 preload 交给页面。同一时间只运行一个 Crew。进程、启动握手与凭证见 [architecture.md](../../docs/architecture.md)，测试与截图命令见 [testing.md](../../docs/testing.md)。

## 入口

| 入口 | 使用方 | 作用 |
|---|---|---|
| `pnpm dev` | 开发者 | 运行 `electron-vite dev --watch`：界面热更新；主进程、preload、Server、Computer 或 `crew` 的源码改动后重新构建并重启 |
| `apps/desktop/electron/main.ts` | Electron | 主进程：单实例锁、启动 Server 与 Computer、创建窗口、出错时弹出对话框并退出 |
| `apps/desktop/electron/preload.ts` | 窗口 | 只向页面暴露 `window.crew.serverUrl`、`window.crew.desktopToken` 与 `window.crew.readMemory` |
| `apps/desktop/electron.vite.config.ts` | 构建 | 一份配置构建主进程、preload 与界面。Server、Computer 与 `crew` 是主进程的额外入口，迁移文件随之复制 |

## 源码地图

| 文件 | 负责 |
|---|---|
| `apps/desktop/electron/runtime.ts` | `startRuntime`：生成 RuntimeSession 与凭证，先启动 Server 再启动 Computer，停止时顺序相反；启动 Computer 时去掉数据库相关的环境变量 |
| `apps/desktop/electron/child.ts` | 启动一个子进程并完成握手：等待 ready 有超时；停止时先 SIGTERM，宽限期后 SIGKILL；保留 stderr 的末尾用于报错。时限见 architecture.md 第 1 节 |
| `apps/desktop/electron/navigation.ts` | 窗口只显示自己的页面，http 与 https 链接交给系统浏览器 |
| `apps/desktop/electron/contract.ts` | 主进程、preload 与界面共用的 IPC 通道名与类型 |
| `apps/desktop/electron/memory.ts` | 读 Agent 的 `MEMORY.md` 给界面看：只读工作目录里的普通文件，至多 256KB |
| `apps/desktop/src/App.tsx` | 两栏布局：侧栏与打开的房间；选中的房间、各个对话框的开关、没有 Agent 时的引导 |
| `apps/desktop/src/components/Sidebar.tsx` | 侧栏：“隐藏侧栏”、“＋ 新建”菜单与连接状态，“消息 / 群聊 / 联系人”分段、会话列表与未读数 |
| `apps/desktop/src/components/Avatar.tsx` | Agent、用户与群聊的头像；群聊头像是成员围成的环 |
| `apps/desktop/src/components/` | 界面组件；`ui/` 下是按 shadcn/ui 做法写的基础组件 |
| `apps/desktop/src/lib/` | 界面逻辑与数据层：`api.ts` 是 Server 客户端，`queries.ts` 用 TanStack Query 读写，`events.ts` 收到 SSE 提示后取新消息或让对应的缓存失效，`messages.ts` 合并分段取到的消息，`mentions.ts` 高亮正文里的 @handle，`avatar.ts` 算头像的颜色与环上的位置，`conversations.ts` 生成会话的预览，`status.ts` 算 Agent 在某个房间的状态，`runs.ts` 是运行记录的写法（结果标签、时长、token、工具调用的一行） |
| `apps/desktop/src/components/SidePanel.tsx` | 右栏：运行记录、讨论串与任务共用的一张卡片，放大后占去聊天区；显示与隐藏侧栏的按钮 |
| `apps/desktop/src/components/MemoryPanel.tsx` | 右栏里私聊对象的记忆，只读 |
| `apps/desktop/src/components/RunPanel.tsx` | 右栏里的运行记录：轮次列表与一轮的时间线，放大后分左右两栏 |
| `apps/desktop/src/components/TaskPanel.tsx` | 右栏里的任务：列表、看板、新建任务与任务详情（状态与负责人的下拉菜单，下面是任务的讨论串） |
| `apps/desktop/src/components/ThreadPanel.tsx` | 右栏里的讨论串：全部讨论串的列表，或一个讨论串的宿主消息、回复与输入框 |
| `apps/desktop/src/components/MessageParts.tsx` | 房间与讨论串共用的消息、通知、讨论串摘要、任务标签、实时活动与输入框 |
| `apps/desktop/src/index.css` | 颜色与字体的设计变量，浅色与深色两套 |
| `apps/desktop/scripts/preview-shot.ts` | `pnpm preview:shot`：用临时数据库启动应用并截图 |
| `apps/desktop/test/support/built-app.ts` | 冒烟测试与真实模型测试共用：启动构建产物 |

## 模型体验

无。

## 已知限制

- **只能用 `pnpm dev` 运行：** 没有 `ELECTRON_RENDERER_URL` 时主进程报错退出，还没有安装包。启动前要先用 Docker 启动 PostgreSQL。
- **重连后消息回到最新一批：** SSE 重连成功时全部缓存失效，每个打开的房间重新取最新 100 条，之前加载的更早消息需要再点“加载更早的消息”。
- **输入 @ 没有自动补全：** 需要照侧栏或成员列表里的 handle 手动输入。
- **Desktop 凭证在页面的 JS 中：** 页面被注入脚本时可以读到它。凭证只在本次运行、只在 loopback 上有效。
