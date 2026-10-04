# Agent Note: Express 5 与 protocol 的接口契约

Status: implemented

## 问题

界面与 Computer 都调用 Server 的 HTTP 接口。修改一个响应字段时，希望所有使用它的地方在类型检查中报错。Server 还要能在关闭时干净地结束 SSE 长连接，不被主进程 SIGKILL。第 1、2 步最初用的是 Hono。

## 决策

- HTTP 框架是 Express 5（`packages/server/src/app.ts`）。
- 接口契约在 `packages/protocol/src/api.ts`：每个接口的方法、路径、参数、请求体与响应的 zod schema。Server 用 `packages/server/src/http.ts` 的 `route()` 按契约注册路由并校验输入，返回值必须符合响应的类型；界面与 Computer 用 `ApiClient` 按契约调用并校验响应。
- SSE 响应手写在 `eventStream` 中，带 `Connection: close`；Server 关闭时主动结束全部 SSE 连接。
- 测试用 `createTestApp` 监听随机端口，请求走真实的 HTTP 连接。

接口列表与错误响应见 [messaging.md](../../../../docs/subsystems/messaging.md) 第 7 节。

## 考虑过的方案

**HTTP 框架用 Hono。** 第 1、2 步用过：`hono/client` 让界面直接得到 Server 路由的类型，测试可以在内存中调用应用。后来换成 Express 5，原因有两个。一是 Hono 的 `streamSSE` 用 `c.header()` 写入 `Connection: keep-alive`，Hono 设置响应时又会用上下文里的头覆盖返回的 Response；SSE 结束后连接空闲地留着，Server 关闭时要等满 5 秒，被主进程 SIGKILL，修正它要绕过这些藏起来的行为。二是 raft 与 cumora 都用 Express，并且手写 SSE 响应（`raft:packages/server/src/routes/internalAgentApi.ts`、`cumora:server/src/agents/runtime/wake-bus.ts`）。

## 后果

- 修改一个 Server 路由的响应字段后，`pnpm typecheck` 在使用该字段的界面与 Computer 代码处报错。
- 走真实 HTTP 连接的测试能测出连接复用一类的问题（`packages/server/test/serve.test.ts`）。
- 契约与 `route()` 是自己维护的一层。SSE 接口不在契约里，路径靠 `EVENT_STREAMS` 常量共享；`crew` 命令直接用 `fetch` 请求 `/agent/reply`，也不受契约保护。
