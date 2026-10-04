# 架构

本文描述 Crew 的进程、包、依赖方向与领域词汇。各部分的行为与数字见子系统页：[消息与接口](subsystems/messaging.md)、[Agent 运行](subsystems/agent-runtime.md)。设计理由见各节末尾链接的 Agent Note，总体计划见[重写的路线图](../.agents/notes/proposed/architecture/2026-10-04-typescript-rewrite.md)。

## 1. 进程与通信

```text
Electron 主进程（监管者）
 ├── 窗口：界面（React）  ── HTTP + SSE ──┐
 ├── Server 子进程         ←───────────────┘  PostgreSQL
 └── Computer 子进程       ── HTTP + SSE ──→  Server
      └── 每轮 Turn 一个 OpenCode 进程（在 Seatbelt 中）
           └── crew（shim）── HTTP ──→  Server
```

- 全部通信都在本机回环地址上。Server 每次启动使用随机端口。
- 主进程先启动 Server，再启动 Computer，两者都就绪后才打开窗口。启动握手：主进程向子进程的 stdin 写一行 JSON（bootstrap，含凭证），子进程就绪后向 stdout 写一行 JSON（ready，Server 的 ready 带回端口）。代码在 `apps/desktop/electron/runtime.ts` 与 `apps/desktop/electron/child.ts`。
- 子进程 30 秒内没有报告 ready 就按启动失败处理。停止时先停 Computer 再停 Server，每个先发 SIGTERM，3 秒后仍未退出就发 SIGKILL。
- 同一时间只运行一个 Crew：再次启动时，新实例直接退出，已有的窗口切到前台（Electron 的单实例锁）。
- 子进程的 stdin 关闭时，子进程退出，所以主进程被强制结束也不会留下孤儿进程。任一子进程意外退出时，主进程弹出错误对话框，停止全部进程后退出。
- 界面经 preload 的 `window.crew` 拿到 Server 地址与凭证，之后直接请求 Server，不经过主进程转发。
- SSE 只传失效提示（“某部分数据变了”），不传业务数据；收到提示的一方重新读取。界面与 Computer 共用 `packages/protocol/src/sse.ts` 的读取与重连。

凭证分三类，互不通用：

| 凭证 | 持有者 | 可以调用 |
|---|---|---|
| Desktop 凭证 | 界面 | `/desktop/*` |
| Computer 凭证 | Computer | `/computer/*` |
| Agent 凭证 | 每个 Agent 一个，存在本次运行目录的文件里 | `/agent/*` |

理由见 [主进程监管 Server 与 Computer](../.agents/notes/implemented/architecture/2026-10-04-process-supervision.md)、[界面直接连接 Server](../.agents/notes/implemented/architecture/2026-10-04-renderer-connects-to-server.md) 与 [SSE 只传失效提示与 Agent 唤醒](../.agents/notes/implemented/architecture/2026-10-04-sse-invalidation-and-wake.md)。

## 2. 包与依赖方向

| 包 | 内容 | 依赖 |
|---|---|---|
| `packages/protocol` | 跨进程的类型与 zod schema、接口契约与 `ApiClient`、branded ID、stdio 握手、SSE 读取 | 无 |
| `packages/server` | Express 接口、drizzle 数据库、进程内事件、运行期状态 | protocol |
| `packages/computer` | Agent 宿主：Runner、OpenCode 适配器、Seatbelt、`~/.crew` 目录、`crew` 命令 | protocol |
| `apps/desktop` | Electron 主进程与 preload（`electron/`）、界面（`src/`） | protocol；测试引用 server 的测试辅助 |

- 包之间直接引用 TypeScript 源码，没有构建步骤。Server、Computer 与 `crew` 由 electron-vite 作为主进程配置的额外入口打包到 `out/main/`，用 Electron 自带的 Node 运行。
- Computer 与界面都不引用 server，只经 HTTP 访问它。接口的方法、路径、参数与响应由 `packages/protocol/src/api.ts` 的契约定义：Server 按它注册路由，返回值必须符合响应的类型；客户端用 `ApiClient` 按它调用并校验响应。
- 一个模块只有一个使用方时并进使用方，例如沙箱代码与 `crew` 都在 `packages/computer` 内。

理由见 [workspace、包划分与构建](../.agents/notes/implemented/architecture/2026-10-04-workspace-and-build.md) 与 [Express 5 与 protocol 的接口契约](../.agents/notes/implemented/architecture/2026-10-04-express-api-contract.md)。

## 3. 数据与状态归属

| 数据 | 位置 | 生命周期 |
|---|---|---|
| 用户、Agent、房间、成员、消息、Agent 已读位置 | PostgreSQL | 持久 |
| Agent 凭证、Agent 状态、可用模型列表 | Server 内存 | 本次运行；应用重启后清空 |
| Agent 的常驻规则、工作目录、OpenCode 数据与 session | `~/.crew/agents/<id>/` | 持久 |
| `crew` 包装脚本、Agent 凭证文件、OpenCode 配置与缓存 | `~/.crew/runtime/<运行 ID>/` | 本次运行；正常退出时删除 |

理由见 [私聊的数据模型](../.agents/notes/implemented/architecture/2026-10-04-direct-chat-data-model.md)。

## 4. 一条消息的路径

1. 用户在界面发消息，界面调用 `POST /desktop/rooms/:roomId/messages`。
2. Server 在一个事务里锁住房间行，分配房间内的序号并写入消息；提交后向界面发“房间有新消息”，向 Computer 发“唤醒”：用户的消息唤醒房间里全部 Agent，Agent 的消息只唤醒它 @ 到的 Agent。
3. Computer 的 Runner 读取 Agent 已读位置之后的消息（同时记为已投递），生成本轮输入，在 Seatbelt 中启动一次 OpenCode。
4. 模型决定回复时运行 `crew reply <room-id>`，正文从 stdin 读入；`crew` 带着 Agent 凭证调用 `POST /agent/reply`。
5. Server 检查房间里有没有已投递位置之后、别人发的消息。有就不写入，把它们返回给 Agent（HELD），Agent 看完再决定；没有就写入回复，向界面发“房间有新消息”。界面只取它缓存之后的新消息。
6. Turn 成功结束后，Runner 确认已读，并上报 Agent 回到空闲。

理由见 [每轮一次 OpenCode 与 crew 命令](../.agents/notes/implemented/architecture/2026-10-04-opencode-turns-and-crew-cli.md) 与 [群聊](../.agents/notes/implemented/feature/2026-10-05-group-chat.md)。

## 5. 领域词汇

| 词 | 含义 |
|---|---|
| User | 使用本机的人。本机只有一个用户。 |
| Agent | 长期存在的 AI 同事：名字、handle、人设、Engine 与模型。 |
| handle | Agent 在消息里被点名用的名字，例如 `@alice`。全局唯一。 |
| Room | 人与 Agent 交流的房间。私聊（direct）：用户与一个 Agent，每个 Agent 一个；群聊（group）：有名字，用户与多个 Agent。 |
| Message | 房间里的一条消息。作者是用户或 Agent；序号在房间内连续递增。 |
| 已读位置 | 每个 Agent 在每个房间处理完的最后一个序号。Turn 成功后推进。 |
| 已投递位置 | 每个 Agent 在每个房间已经看过的最后一个序号。读取 inbox 与 HELD 时推进。 |
| HELD | Agent 的回复因为房间里有它没看过的新消息而没有发出；它看完新消息后再决定。 |
| RuntimeSession | 一次应用运行。它的 ID 用于本次运行目录；凭证与运行期状态只在本次运行内有效。 |
| Computer | 在本机运行 Agent 的进程。 |
| Engine | 实际调用模型的程序。目前只有 OpenCode。 |
| Turn | Agent 被唤醒后的一次运行：读取未读消息，运行一次 Engine。 |
| Runner | Computer 中每个 Agent 一个的串行循环，负责运行 Turn。 |
| shim | Agent 用来发言的命令 `crew`。Agent 的纯文本输出没有人看到。 |
| 失效提示 | SSE 推送的事件，只说明哪部分数据变了。 |
