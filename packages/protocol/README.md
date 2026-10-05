# @crew/protocol

跨进程共用的类型与校验：接口契约、branded ID、启动握手的消息与 SSE 读取，以及各包共用的 `assertNever`。Server、Computer、`crew` 命令与界面都依赖它，它不依赖其他 workspace 包。它同时运行在 Node 与浏览器中，只能使用两边都有的 API：根目录 `biome.json` 禁止本包使用 `window`、`process`、`Buffer` 等全局变量。接口的行为见 [messaging.md](../../docs/subsystems/messaging.md)，进程关系见 [architecture.md](../../docs/architecture.md)。

## 入口

`package.json` 的 `exports` 指向 `src/index.ts`，没有构建步骤。

| 导出 | 使用方 | 作用 |
|---|---|---|
| `api`、`Endpoint` | Server 注册路由；界面与 Computer 调用 | 每个接口的方法、路径、参数、请求体与响应的 zod schema。改一个响应字段时改这里，Server 与客户端一起在类型检查中报错 |
| `ApiClient`、`ApiError` | 界面、Computer | 按契约发请求，按响应 schema 校验结果；Server 拒绝时抛出 `ApiError` |
| `EVENT_STREAMS`、`DesktopEvent`、`ComputerEvent` | Server、界面、Computer | 两个 SSE 接口的路径与事件的 schema。SSE 不在 `api` 契约里 |
| `runEventStream`、`DEFAULT_BACKOFF` | 界面、Computer | 读取 SSE，断开后按指数退避重连 |
| `ServerBootstrap`、`ServerReady`、`ComputerBootstrap`、`ComputerReady`、`readMessage`、`encodeMessage` | Desktop 主进程、Server、Computer | 启动握手：stdin 上一行 JSON 的 bootstrap，stdout 上一行 JSON 的 ready |
| `UserId`、`AgentId`、`RoomId`、`MessageId`、`RuntimeSessionId` | 全部 | branded ID |
| `MESSAGE_BODY_MAX`、`DISPLAY_NAME_MAX`、`PERSONA_MAX`、`ROOM_NAME_MAX`、`Handle` | Server、界面、`crew` 命令 | 消息正文、Agent 名字与人设、群聊名字的长度上限与 handle 的格式，各方用同一份规则 |
| `Conversation` | Server、界面 | 会话列表的一项：最后一条消息与未读数 |
| `mentionPattern`、`normalizeHandle` | Server、界面 | 正文里 `@handle` 的写法：Server 据此记录点名，界面据此高亮 |
| `ReplyOutcome` | Server、`crew` 命令 | `POST /agent/reply` 的结果：发出，或被 HELD 拦下并附上新消息 |
| `assertNever` | 界面；其他包需要时从这里导入 | 封闭联合的 `switch` 用它结尾：漏掉一个分支时，类型检查在这里报错 |

## 源码地图

| 文件 | 负责 |
|---|---|
| `packages/protocol/src/api.ts` | 接口契约 `api`，以及由契约推导参数、查询参数、请求体与响应类型 |
| `packages/protocol/src/client.ts` | `ApiClient` |
| `packages/protocol/src/collab.ts` | handle、房间、参与者、Agent 状态、消息、inbox、回复结果与 SSE 事件的 schema |
| `packages/protocol/src/runs.ts` | 运行记录：Engine 事件、运行事件、用量、一轮的概要与详情 |
| `packages/protocol/src/ids.ts` | 数据库实体的 branded ID |
| `packages/protocol/src/runtime.ts` | `RuntimeSessionId` 与启动握手的四种消息 |
| `packages/protocol/src/stdio.ts` | 读一行 JSON、写一行 JSON |
| `packages/protocol/src/sse.ts` | SSE 读取与重连 |
| `packages/protocol/src/assert.ts` | `assertNever` |

## 模型体验

间接：`MESSAGE_BODY_MAX` 限制 Agent 用 `crew reply` 发出的正文长度，见 [@crew/computer](../computer/README.md)。任务的状态名、流转表与拒绝原因（`src/tasks.ts`）出现在 `crew task` 的输出里。

## 已知限制

- **接口契约是自己维护的一层：** Server 用 `packages/server/src/http.ts` 的 `route()` 按契约注册路由。契约之外的调用不受类型检查保护：SSE 的路径只靠 `EVENT_STREAMS` 共享，`crew` 命令直接用 `fetch` 请求 `/agent/reply`，只用 `ReplyOutcome` 校验响应。
