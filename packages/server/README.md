# @crew/server

Collaboration Server：保存用户、Agent、房间与消息，提供界面、Computer 与 Agent 三组 HTTP 接口，并用 SSE 推送失效提示。它由 Desktop 主进程作为子进程启动，只监听 `127.0.0.1` 的随机端口。只有它访问 PostgreSQL 与 Redis。数据模型、接口、环境变量与验收见 [messaging.md](../../docs/subsystems/messaging.md)。

## 入口

| 入口 | 使用方 | 作用 |
|---|---|---|
| `packages/server/src/main.ts` | Desktop 主进程（electron-vite 把它构建为主进程旁的 `server.js`） | 进程入口：读环境变量与 stdin 上的 bootstrap，执行迁移，监听后向 stdout 写 ready |
| `createApp`（`@crew/server`） | `main.ts` 与测试 | 构建 Express 应用。依赖全部由参数传入，不读环境变量 |
| `@crew/server/testing` | 本包、Computer 与 Desktop 的测试 | `createTestApp`（真实路由与临时数据库，监听随机端口）、`createTestDatabase`、`testEnv` 与测试用凭证 |
| `pnpm --filter @crew/server db:generate` | 开发者 | 改了 `packages/server/src/db/schema.ts` 后，用 drizzle-kit 生成迁移 |

## 源码地图

| 文件 | 负责 |
|---|---|
| `packages/server/src/main.ts` | 进程入口：连接 PostgreSQL 与 Redis、迁移、握手、关闭 |
| `packages/server/src/app.ts` | 中间件的顺序：CORS、三类凭证、JSON 请求体，然后挂上路由 |
| `packages/server/src/http.ts` | 凭证校验、按契约注册接口的 `route()`、SSE 响应、把错误转成 `{ error }` |
| `packages/server/src/routes/` | 三组接口：`desktop.ts`、`computer.ts`、`agent.ts` |
| `packages/server/src/agents.ts` | 新建 Agent（连同私聊房间与成员关系）与列出 Agent |
| `packages/server/src/messages.ts` | 写入消息与分配序号、读取消息、inbox 与已读位置 |
| `packages/server/src/context.ts` | 路由的依赖；消息写入后通知界面与唤醒 Agent |
| `packages/server/src/events.ts` | 进程内事件总线 `EventHub` |
| `packages/server/src/state.ts` | 只在内存中的 Agent 凭证、Agent 状态与模型列表 |
| `packages/server/src/db/` | drizzle 表结构（`schema.ts`）、执行迁移、本机用户 |
| `packages/server/src/serve.ts` | 监听随机端口；关闭时限时等待正在进行的请求 |
| `packages/server/drizzle/` | drizzle-kit 生成的迁移，提交进仓库 |

## 模型体验

间接：Agent 每轮看到的消息正文、作者的显示名与本机用户名 “User” 来自这里的数据，由 [@crew/computer](../computer/README.md) 写进每轮输入。

## 已知限制

- **只支持单个 Server 进程：** 唤醒与 SSE 提示走进程内的 `EventHub`，Agent 凭证与状态也只在本进程的内存中。改为多实例部署时，这些都要移到 Redis。
