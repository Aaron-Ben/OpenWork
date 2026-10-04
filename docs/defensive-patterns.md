# 防御性写法

本文只记录本仓库实际出现过的缺陷，每条写现象、规则与出处。写子进程、流、取消与清理代码时对照它，评审时也一样。更完整的清单见 DSH 的 `docs/defensive-patterns.md`，本文不照搬它。

## 中止由自己执行到底

- **现象：** 测试中止 SSE 连接后，读取循环没有返回，测试一直挂着。真实的 `fetch` 在中止时会关闭响应流，测试用的内存 fetch 不会，`reader.read()` 于是一直等下去。
- **规则：** 收到中止信号时，自己关闭自己持有的资源（取消 reader、结束子进程、清除计时器），不依赖下层替你关闭。
- **出处：** `packages/protocol/src/sse.ts` 的 `readOnce`，中止时调用 `reader.cancel()`。

## 订阅信号后立刻检查当前状态

- **现象：** 中止发生在“连接已建立、还没注册 abort 监听”之间时，监听永远不会触发，读取又会挂住；等待重连的 `sleep` 有同样的问题。
- **规则：** `addEventListener("abort")` 只对之后发生的中止生效。注册监听后立刻检查一次 `signal.aborted`；进入任何等待之前也先检查。
- **出处：** `packages/protocol/src/sse.ts` 的 `readOnce` 与 `sleep`。

## 两个事件的到达顺序没有保证

- **现象：** Server 启动失败时，错误对话框有时只显示“输入意外结束”。子进程退出时 stdout 往往先关闭，`exit` 事件稍后才到，代码先看到 stdout 结束，就报了一个看不出原因的错误。
- **规则：** 同一件事会触发多个事件时，不假设它们的顺序；明确等待信息更多的那个，并给等待设上限。
- **出处：** `apps/desktop/electron/child.ts` 的 `EXIT_AFTER_EOF_MS`：stdout 结束后最多再等 500 毫秒的 `exit`，等到就报告退出码。

## 注入的依赖要一路传到每个使用点

- **现象：** 测试给 `ServerClient` 注入了内存 fetch，Computer 订阅 SSE 时却用了全局 `fetch`，连向一个不存在的地址。测试没有失败，只是悄悄少测了一段。
- **规则：** 注入的依赖（fetch、时钟、IO）要传到每一个使用点，不能有路径回落到全局默认值。加一个测试，让回落时测试失败。
- **出处：** `packages/computer/src/client.ts` 的 `eventStream()` 返回 `fetch: this.fetchFn`。去掉它，`packages/computer/test/daemon.test.ts` 的 6 个测试中有 5 个失败。
