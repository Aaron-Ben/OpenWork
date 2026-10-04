# 防御性写法

本文只记录本仓库实际出现过的缺陷，每条写现象、规则与出处。写子进程、流、取消与清理代码时对照它，评审时也一样。更完整的清单见 DSH 的 `dsh:docs/defensive-patterns.md`，本文不照搬它。

## 中止由自己执行到底

- **现象：** 测试中止 SSE 连接后，读取循环没有返回，测试一直挂着。真实的 `fetch` 在中止时会关闭响应流，测试用的内存 fetch 不会，`reader.read()` 于是一直等下去。
- **规则：** 收到中止信号时，自己关闭自己持有的资源（取消 reader、结束子进程、清除计时器），不依赖下层替你关闭。
- **出处：** `packages/protocol/src/sse.ts` 的 `readOnce`，中止时调用 `reader.cancel()`。

## 订阅信号后立刻检查当前状态

- **现象：** 中止发生在“连接已建立、还没注册 abort 监听”之间时，监听永远不会触发，读取又会挂住；等待重连的 `sleep` 有同样的问题。
- **规则：** `addEventListener("abort")` 只对之后发生的中止生效。注册监听后立刻检查一次 `signal.aborted`；进入任何等待、启动任何子进程之前也先检查。同理，“进程已经退出”这类已经发生的事件，要补报给之后才注册的监听。
- **出处：** `packages/protocol/src/sse.ts` 的 `readOnce` 与 `sleep`。同一类问题后来又出现两次：`packages/computer/src/engine/opencode.ts` 的 `runOnce` 在准备期间被停止时照样启动 OpenCode，进程组无人结束；`apps/desktop/electron/child.ts` 的 `onUnexpectedExit` 在子进程退出之后注册时收不到通知。

## 两个事件的到达顺序没有保证

- **现象：** Server 启动失败时，错误对话框有时只显示“输入意外结束”。子进程退出时 stdout 往往先关闭，`exit` 事件稍后才到，代码先看到 stdout 结束，就报了一个看不出原因的错误。
- **规则：** 同一件事会触发多个事件时，不假设它们的顺序；明确等待信息更多的那个，并给等待设上限。
- **出处：** `apps/desktop/electron/child.ts` 的 `EXIT_AFTER_EOF_MS`：stdout 结束后最多再等 500 毫秒的 `exit`，等到就报告退出码。

## 注入的依赖要一路传到每个使用点

- **现象：** 测试给 `ServerClient` 注入了内存 fetch，Computer 订阅 SSE 时却用了全局 `fetch`，连向一个不存在的地址。测试没有失败，只是悄悄少测了一段。
- **规则：** 注入的依赖（fetch、时钟、IO）要传到每一个使用点，不能有路径回落到全局默认值。加一个测试，让回落时测试失败。
- **出处：** `packages/computer/src/client.ts` 的 `eventStream()` 返回 `fetch: this.fetchFn`。去掉它，`packages/computer/test/daemon.test.ts` 的 6 个测试中有 5 个失败。

## 停止要在宽限期内完成

- **现象：** Computer 停止时先等 SSE 与同步结束才去中止 Runner，Server 关闭时等界面的 SSE 长连接自己断开。两者都可能超过主进程给的 3 秒，进程被 SIGKILL：Engine 成为孤儿，数据库连接被硬断。
- **规则：** 停止时先同时发出全部中止，再等待它们结束；长连接由服务端主动结束，不等客户端，结束后连接也要关闭，不能以 keep-alive 的形式空闲着拖住 `server.close()`。估算停止的最长耗时，确认它小于上一级给的宽限期，并用真实的连接测一次：内存中的请求测不出连接复用的问题。
- **出处：** `packages/computer/src/daemon.ts` 的 `stop`；`packages/server/src/events.ts` 的 `Channel.close`；`packages/server/src/http.ts` 的 `eventStream` 设置 `Connection: close`；`packages/server/test/serve.test.ts`。

## 约定不抛出的接口要接住全部异常

- **现象：** `EngineAdapter.runTurn` 约定用返回值报告失败，但登录文件读不了时直接抛出。Runner 已经上报了“回复中”，异常被吞掉后不再上报，界面一直显示“回复中”。
- **规则：** 实现方把全部意外错误转成约定的失败结果；调用方也为违约兜底，失败时把状态改到能看见原因的地方。后台准备失败时同样要让失败被看见：退出或上报，不能只写日志。
- **出处：** `packages/computer/src/engine/opencode.ts` 的 `runTurn`；`packages/computer/src/runner.ts` 的 `run`；`packages/computer/src/main.ts` 在 `daemon.start()` 失败时退出。

## 不顺着不可信方控制的路径操作

- **现象：** Agent 在沙箱里能写自己的目录。它可以把 `work/` 换成指向 `$HOME` 别处的符号链接，Computer 重建它的 Runner 时就会在沙箱外建目录、改权限；把 `session.json` 换成命名管道，Computer 读它时会永远等下去。
- **规则：** 不受沙箱约束的一方，操作不可信方能写的路径之前，逐级用 `lstat` 确认是真正的目录或普通文件，遇到符号链接与特殊文件就拒绝，并把原因上报。
- **出处：** `packages/computer/src/home.ts` 的 `ensureDirUnder` 与 `resumableSession`；`packages/computer/src/daemon.ts` 给准备失败的 Agent 上报 error。

## 浏览器 API 要以原来的方式调用

- **现象：** 接口客户端把全局的 `fetch` 存成自己的属性，再用 `this.fetchFn(...)` 调用。Node 里一切正常，测试全部通过；界面里却报 “Failed to execute 'fetch' on 'Window': Illegal invocation”，Agent 列表读不出来。浏览器的 `fetch` 只能以 `window` 为 `this` 调用。
- **规则：** 存起来的 `fetch` 一类浏览器 API，先取到局部变量再当作普通函数调用。只在 Node 里跑的测试测不出这一类问题，改了界面用到的代码要在真实界面里跑一次（`pnpm preview:shot`）。
- **出处：** `packages/protocol/src/client.ts` 的 `ApiClient.call`；`packages/protocol/test/client.test.ts` 用一个检查 `this` 的假 `fetch` 防止回退。

## 进程退出后清理它留下的进程

- **现象：** OpenCode 正常结束一轮后，Agent 在这一轮里用 `cmd &` 起的后台进程还活着，在沙箱里继续联网；后台进程如果继承了 OpenCode 的 stdout，管道一直不关，`close` 事件不来，这一轮卡到后台进程自己退出为止。cumora 的注释记录了同样的现象，但只在中止时处理（`cumora:server/src/agents/computer/engine.ts` 的 `spawnEngine`）。
- **规则：** 子进程退出（`exit` 事件）后，结束它所在的整个进程组，再等 `close`。不要只靠 `close` 判断一轮结束。
- **出处：** `packages/computer/src/engine/opencode.ts` 的 `runOnce`；`packages/computer/test/opencode.test.ts` 的 `ends processes the agent left running when the turn finishes`。

## 沙箱里验证模型真正会用的写法

- **现象：** 常驻规则教 Agent 用 heredoc 运行 `crew reply`，真实模型照做时，沙箱里的 zsh 报 `can't create temp file for here document: operation not permitted`，模型只好改用 `printf`。zsh 把 heredoc 写进以 `TMPPREFIX` 开头的临时文件，默认是沙箱不让写的 `/tmp/zsh`。冒烟测试的假 opencode 直接启动 `crew` 并写 stdin，没有经过 shell，所以没测出来。
- **规则：** 提示词教给模型的写法，要在同样的沙箱、同样的环境变量下真正跑一遍，不只测被调用的程序本身。
- **出处：** `packages/computer/src/engine/opencode.ts` 设置 `TMPPREFIX`；`packages/computer/test/opencode.test.ts` 的 `lets the shell inside the sandbox run a heredoc, as the standing instructions tell the agent to`，去掉 `TMPPREFIX` 时它报出同样的错误。2026-10-05 用真实模型演示群聊时发现。
