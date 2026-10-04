# 架构

本文描述 Crew 的进程、包、依赖方向与领域词汇。各部分的行为与数字见子系统页：[消息与接口](subsystems/messaging.md)、[Agent 运行](subsystems/agent-runtime.md)。设计理由见[重写的 Agent Note](../.agents/notes/proposed/architecture/2026-10-04-typescript-rewrite.md)。

## 1. 进程与通信

```text
Electron 主进程（监管者）
 ├── 窗口：界面（React）  ── HTTP + SSE ──┐
 ├── Server 子进程         ←───────────────┘  PostgreSQL、Redis
 └── Computer 子进程       ── HTTP + SSE ──→  Server
      └── 每轮 Turn 一个 OpenCode 进程（在 Seatbelt 中）
           └── crew（shim）── HTTP ──→  Server
```

- 全部通信都在本机回环地址上。Server 每次启动使用随机端口。
- 主进程先启动 Server，再启动 Computer，两者都就绪后才打开窗口。启动握手：主进程向子进程的 stdin 写一行 JSON（bootstrap，含凭证），子进程就绪后向 stdout 写一行 JSON（ready，Server 的 ready 带回端口）。代码在 `apps/desktop/electron/runtime.ts` 与 `apps/desktop/electron/child.ts`。
- 子进程的 stdin 关闭时，子进程退出，所以主进程被强制结束也不会留下孤儿进程。任一子进程意外退出时，主进程弹出错误对话框，停止全部进程后退出。
- 界面经 preload 的 `window.crew` 拿到 Server 地址与凭证，之后直接请求 Server，不经过主进程转发。
- SSE 只传失效提示（“某部分数据变了”），不传业务数据；收到提示的一方重新读取。界面与 Computer 共用 `packages/protocol/src/sse.ts` 的读取与重连。

凭证分三类，互不通用：

| 凭证 | 持有者 | 可以调用 |
|---|---|---|
| Desktop 凭证 | 界面 | `/desktop/*` |
| Computer 凭证 | Computer | `/computer/*` |
| Agent 凭证 | 每个 Agent 一个，存在本次运行目录的文件里 | `/agent/*` |

## 2. 包与依赖方向

| 包 | 内容 | 依赖 |
|---|---|---|
| `packages/protocol` | 跨进程的类型与 zod schema、branded ID、stdio 握手、SSE 读取 | 无 |
| `packages/server` | Hono 接口、drizzle 数据库、进程内事件、运行期状态 | protocol |
| `packages/computer` | Agent 宿主：Runner、OpenCode 适配器、Seatbelt、`~/.crew` 目录、`crew` 命令 | protocol |
| `apps/desktop` | Electron 主进程与 preload（`electron/`）、界面（`src/`） | protocol；只引用 server 的类型 |

- 包之间直接引用 TypeScript 源码，没有构建步骤。Server、Computer 与 `crew` 由 electron-vite 作为主进程配置的额外入口打包到 `out/main/`，用 Electron 自带的 Node 运行。
- Computer 不引用 server，只经 HTTP 访问它。界面只引用 server 的 `AppType`，`hono/client` 由此得到有类型的接口。
- 一个模块只有一个使用方时并进使用方，例如沙箱代码与 `crew` 都在 `packages/computer` 内。

## 3. 数据与状态归属

| 数据 | 位置 | 生命周期 |
|---|---|---|
| 用户、Agent、房间、成员、消息、Agent 已读位置 | PostgreSQL | 持久 |
| Agent 凭证、Agent 状态、可用模型列表 | Server 内存 | 本次运行；应用重启后清空 |
| Agent 的常驻规则、工作目录、OpenCode 数据与 session | `~/.crew/agents/<id>/` | 持久 |
| `crew` 包装脚本、Agent 凭证文件、OpenCode 配置与缓存 | `~/.crew/runtime/<运行 ID>/` | 本次运行；正常退出时删除 |

Redis 目前只在启动时检查连接，还没有读写。

## 4. 一条消息的路径

1. 用户在界面发消息，界面调用 `POST /desktop/rooms/:roomId/messages`。
2. Server 在一个事务里锁住房间行，分配房间内的序号并写入消息；提交后向界面发“房间有新消息”，向 Computer 发“唤醒这个 Agent”。
3. Computer 的 Runner 读取 Agent 已读位置之后的消息，生成本轮输入，在 Seatbelt 中启动一次 OpenCode。
4. 模型决定回复时运行 `crew reply <room-id>`，正文从 stdin 读入；`crew` 带着 Agent 凭证调用 `POST /agent/reply`。
5. Server 写入回复，向界面发“房间有新消息”，界面重新读取并显示。
6. Turn 成功结束后，Runner 确认已读，并上报 Agent 回到空闲。

## 5. 领域词汇

| 词 | 含义 |
|---|---|
| User | 使用本机的人。本机只有一个用户。 |
| Agent | 长期存在的 AI 同事：名字、人设、Engine 与模型。 |
| Room | 人与 Agent 交流的房间。目前只有私聊（direct）：每个 Agent 一个。 |
| Message | 房间里的一条消息。作者是用户或 Agent；序号在房间内连续递增。 |
| 已读位置 | 每个 Agent 在每个房间读到的最后一个序号。只前进，不后退。 |
| RuntimeSession | 一次应用运行。它的 ID 用于本次运行目录；凭证与运行期状态只在本次运行内有效。 |
| Computer | 在本机运行 Agent 的进程。 |
| Engine | 实际调用模型的程序。目前只有 OpenCode。 |
| Turn | Agent 被唤醒后的一次运行：读取未读消息，运行一次 Engine。 |
| Runner | Computer 中每个 Agent 一个的串行循环，负责运行 Turn。 |
| shim | Agent 用来发言的命令 `crew`。Agent 的纯文本输出没有人看到。 |
| 失效提示 | SSE 推送的事件，只说明哪部分数据变了。 |
