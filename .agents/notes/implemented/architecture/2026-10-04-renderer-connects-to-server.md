# Agent Note: 界面直接连接 Server

Status: implemented

## 问题

界面要调用 Server 的接口并接收 SSE。Server 每次启动使用随机端口与临时凭证，所以地址与凭证只能在运行时交给界面。raft 与 cumora 在构建时写入 API 地址，因为它们连接固定的云端地址，这种做法在这里不成立。需要决定界面怎样拿到地址与凭证、请求走哪条路，以及凭证怎样保护。

## 决策

- 界面直接调用 Server，用 protocol 的 `ApiClient` 按接口契约调用，请求带 `Authorization: Bearer <Desktop 凭证>`（`apps/desktop/src/lib/api.ts`）。
- SSE 用 `fetch` 读取，凭证放在请求头。浏览器的 `EventSource` 不能设置请求头。
- preload 用 `ipcRenderer.sendSync` 向主进程索取一次地址与凭证，再经 `contextBridge` 只向页面暴露 `window.crew.serverUrl` 与 `window.crew.desktopToken`（`apps/desktop/electron/preload.ts`）。凭证不经过 `additionalArguments`：那会把它放进渲染进程的命令行参数，本机其他用户可以用 `ps` 看到。
- 窗口保持 Electron 的默认安全设置：上下文隔离、关闭 Node 集成、启用沙箱。
- Server 的 CORS 只允许界面的来源调用 `/desktop/*`。
- 页面的 Content-Security-Policy 只允许本页面的资源、对 `127.0.0.1` 任意端口的请求，以及开发服务器的热更新连接（`apps/desktop/src/index.html`）。

凭证的种类与作用范围见 [architecture.md](../../../../docs/architecture.md) 第 1 节。

## 考虑过的方案

**界面经主进程转发调用 Server。** Tauri 版这样做，凭证不进入页面。没有采用：每个接口多一层 IPC，接口契约的端到端类型要在 IPC 上再接一次。页面被攻击时，攻击者同样可以让主进程替它转发请求，转发带来的隔离有限。

**由主进程拦截请求并自动加上凭证。** Electron 的 `session.webRequest` 可以修改页面发出的请求头，凭证不进入页面，`ApiClient` 仍然可用。没有采用：页面代码看不出凭证从哪里来，排查与讲解都更难。

**把凭证放在 SSE 的 URL 中。** 可以继续用浏览器的 `EventSource`。没有采用：凭证会出现在 URL 与日志中。

## 后果

- 界面与 Computer 用同一个 `ApiClient` 与同一份 SSE 读取代码。
- Desktop 凭证存在页面的 JS 中。页面被注入脚本时，凭证可以被读取。凭证只在当前 RuntimeSession 内、只在 loopback 上有效。
- 浏览器的 `fetch` 只能以 `window` 为 `this` 调用，存起来的 `fetch` 要当作普通函数调用，见 [defensive-patterns.md](../../../../docs/defensive-patterns.md)“浏览器 API 要以原来的方式调用”。
