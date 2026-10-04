# @crew/desktop

Electron 应用：主进程启动并监管 Server 与 Computer，窗口里是 React 界面。界面直接用 HTTP 与 SSE 连接 Server，不经主进程转发；Server 地址与 Desktop 凭证由 preload 交给页面。同一时间只运行一个 Crew。进程、启动握手与凭证见 [architecture.md](../../docs/architecture.md)，测试与截图命令见 [testing.md](../../docs/testing.md)。

## 入口

| 入口 | 使用方 | 作用 |
|---|---|---|
| `pnpm dev` | 开发者 | 运行 `electron-vite dev --watch`：界面热更新；主进程、preload、Server、Computer 或 `crew` 的源码改动后重新构建并重启 |
| `apps/desktop/electron/main.ts` | Electron | 主进程：单实例锁、启动 Server 与 Computer、创建窗口、出错时弹出对话框并退出 |
| `apps/desktop/electron/preload.ts` | 窗口 | 只向页面暴露 `window.crew.serverUrl` 与 `window.crew.desktopToken` |
| `apps/desktop/electron.vite.config.ts` | 构建 | 一份配置构建主进程、preload 与界面。Server、Computer 与 `crew` 是主进程的额外入口，迁移文件随之复制 |

## 源码地图

| 文件 | 负责 |
|---|---|
| `apps/desktop/electron/runtime.ts` | `startRuntime`：生成 RuntimeSession 与凭证，先启动 Server 再启动 Computer，停止时顺序相反；启动 Computer 时去掉数据库相关的环境变量 |
| `apps/desktop/electron/child.ts` | 启动一个子进程并完成握手：等待 ready 有超时；停止时先 SIGTERM，宽限期后 SIGKILL；保留 stderr 的末尾用于报错。时限见 architecture.md 第 1 节 |
| `apps/desktop/electron/navigation.ts` | 窗口只显示自己的页面，http 与 https 链接交给系统浏览器 |
| `apps/desktop/electron/contract.ts` | 主进程、preload 与界面共用的 IPC 通道名与 `RendererRuntime` 类型 |
| `apps/desktop/src/App.tsx` | 两栏布局：Agent 列表与聊天 |
| `apps/desktop/src/components/` | 界面组件；`ui/` 下是按 shadcn/ui 做法写的基础组件 |
| `apps/desktop/src/lib/` | 界面逻辑与数据层：`api.ts` 是 Server 客户端，`queries.ts` 用 TanStack Query 读写，`events.ts` 收到 SSE 提示后让对应的缓存失效 |
| `apps/desktop/src/index.css` | 颜色与字体的设计变量，浅色与深色两套 |
| `apps/desktop/scripts/preview-shot.ts` | `pnpm preview:shot`：用临时数据库启动应用并截图 |
| `apps/desktop/test/support/built-app.ts` | 冒烟测试与真实模型测试共用：启动构建产物 |

## 模型体验

无。

## 已知限制

- **只能用 `pnpm dev` 运行：** 没有 `ELECTRON_RENDERER_URL` 时主进程报错退出，还没有安装包。启动前要先用 Docker 启动 PostgreSQL。
- **每次提示都重新获取整个消息列表：** 收到“房间有新消息”时，界面重新读取这个房间的全部消息。消息多时要改为只取某个序号之后的消息。
- **Desktop 凭证在页面的 JS 中：** 页面被注入脚本时可以读到它。凭证只在本次运行、只在 loopback 上有效。
