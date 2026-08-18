# OpenWork Collaboration P0 — API research notes

> 调查日期：2026-08-18。本文只整理实现五个 P0 尖刺所需的一手资料，不包含运行结果，也不能替代同目录 `SPIKE-P0.md` 的实测证据。

## 0. 资料边界与版本警告

- 仓库约束要求只做最小的端到端尖刺、不保留过时路径、不加兼容层（`/Volumes/Extreme SSD/Code/OpenWork/AGENTS.md:1`）。设计明确规定 P0 不写业务代码，且只用非 `/experimental/` 端点（`/Volumes/Extreme SSD/Code/OpenWork/docs/collaboration.md:108`、`:396`）。
- 权威 API 文件是 `/Volumes/Extreme SSD/Code/opencode/packages/sdk/openapi.json`。它能被 `jq` 完整解析为 OpenAPI 3.1，并恰有 162 个 path；以下 API 契约均引用该文件的 JSON Pointer。
- 本机安装的可执行文件是 1.18.18，但本地 OpenCode checkout 当前 `packages/opencode/package.json` 是 1.18.4，且 `git describe` 为 `github-v1.2.25-1392-g62e4641235`。因此：
  - `openapi.json` 按任务要求视为权威接口规格；
  - checkout 源码只用于解释实现机制、形成待验证假设；
  - 任何 1.18.18 的行为结论都必须由 bin 实测，不能把源码推断写成“通过”。

## 1. 路径族结论：P0 应使用 `/session/*`

权威规格同时包含两套路由，但 P0 指定的异步 prompt 只存在于旧/稳定的 `/session/*` 路径族：

| 能力 | 非 experimental 路径 | OpenAPI 位置 |
|---|---|---|
| 建 session | `POST /session` | `#/paths/~1session/post`，文件 `:5290` |
| 取 session | `GET /session/{sessionID}` | `#/paths/~1session~1{sessionID}/get`，文件 `:5577` |
| 发异步 prompt | `POST /session/{sessionID}/prompt_async` | `#/paths/~1session~1{sessionID}~1prompt_async/post`，文件 `:7102` |
| 目录级 SSE | `GET /event` | `#/paths/~1event/get`，文件 `:576` |
| 真正的全 server SSE | `GET /global/event` | `#/paths/~1global~1event/get`，文件 `:324` |
| 待决审批 | `GET /permission` | `#/paths/~1permission/get`，文件 `:4811` |
| 回复审批 | `POST /permission/{requestID}/reply` | `#/paths/~1permission~1{requestID}~1reply/post`，文件 `:4869` |
| MCP 连接状态（诊断用） | `GET /mcp` | `#/paths/~1mcp/get`，文件 `:2968` |
| 健康/版本 | `GET /global/health` | `#/paths/~1global~1health/get`，文件 `:275` |

`/api/session` 确实存在（文件 `:9985`），但它属于 v2 API；该路径族的发送端点是 `/api/session/{sessionID}/prompt`，没有 `prompt_async`。所以五个尖刺若按题目要求使用 `prompt_async`，必须实际走 `/session/*`。

一个已确认的设计/API 落差：`docs/collaboration.md:126` 列出的 `GET /session/{id}/event` 在权威规格中不存在。规格里只有：

- `GET /event`；
- `GET /global/event`；
- v2 的 `GET /api/session/{sessionID}/event`（文件 `:11224`）。

P0 不应为了贴合文档虚构 `/session/{id}/event`，也不应混用 v2 session event；使用同一 directory 的 `GET /event`，再按 `properties.sessionID` 客户端过滤。

## 2. directory 路由与 server 启动

### 2.1 `x-opencode-directory` 的实际语义

OpenAPI 对 instance 端点统一公开可选 query `directory` 与 `workspace`，例如 `#/paths/~1session/post/parameters` 和 `#/paths/~1event/get/parameters`。任务要求使用的 header 是 server/SDK 的等价路由入口：

- OpenCode instance 路由优先读取 `directory` query，其次读取 `x-opencode-directory`，最后才回退到 server 进程 cwd（`/Volumes/Extreme SSD/Code/opencode/packages/opencode/src/server/routes/instance/httpapi/middleware/workspace-routing.ts:86`）；加载 instance 前会 `decodeURIComponent`（`/Volumes/Extreme SSD/Code/opencode/packages/opencode/src/server/routes/instance/httpapi/middleware/instance-context.ts:15`）；
- JS SDK 在配置 `directory` 后把 `encodeURIComponent(directory)` 放入 `x-opencode-directory`（`/Volumes/Extreme SSD/Code/opencode/packages/sdk/js/src/client.ts:33`）；对 GET/HEAD，它会改写成 query `directory` 并删掉 header（同文件 `:17`）。

因此 Rust 尖刺应：

1. 创建绝对、最好已 canonicalize 的临时目录；
2. 对 `POST /session`、`GET /event`、两个 prompt、审批 list/reply 等所有 instance 请求使用同一个 `x-opencode-directory`；
3. SSE 必须同样带该 directory，否则会订阅到 server cwd 对应的另一个 instance；
4. header 可以使用 URL 编码值（与官方 SDK 一致）；server 对未编码的合法 ASCII path 也能接受，因为 decode 失败或无转义时原样使用。

### 2.2 `/event` 并非跨 directory 的“全局流”

当前 server 源码会按 instance directory/workspace 过滤 `/event`（`/Volumes/Extreme SSD/Code/opencode/packages/opencode/src/server/routes/instance/httpapi/handlers/event.ts:25`）。真正跨 instance 的 `/global/event` 返回：

```json
{
  "directory": "/absolute/agent/home",
  "payload": { "id": "evt_...", "type": "...", "properties": {} }
}
```

契约见 `#/components/schemas/GlobalEvent`。这意味着设计里“一个 server 服务多个 directory”成立，但 `/event` 和 `GET /permission` 都是 directory instance 视角；所谓“全局”至多是该 directory 下所有 session。P0 只有一个 directory，不受影响；未来若要一个角标汇总所有 Agent，不能只对 server cwd 调一次 `GET /permission`，需要按 Agent directory 聚合，或用 `/global/event` 维护待决集合再逐 directory 查询。

### 2.3 启动和 Basic auth

- 当前 CLI 默认 `port=0`、`hostname=127.0.0.1`（`/Volumes/Extreme SSD/Code/opencode/packages/opencode/src/cli/network.ts:6`）。
- 就绪行是 `opencode server listening on http://${hostname}:${port}`（`/Volumes/Extreme SSD/Code/opencode/packages/opencode/src/cli/cmd/serve.ts:13`）。bin 应读子进程 stdout 到这行后再发请求；不应猜端口。
- 未设置密码时授权中间件直接放行；设置密码时必须发 `Authorization: Basic base64(username:password)`，username 默认 `opencode`（`/Volumes/Extreme SSD/Code/opencode/packages/server/src/auth.ts:40`、`/Volumes/Extreme SSD/Code/opencode/packages/server/src/middleware/authorization.ts:29`）。
- P0 环境声明未设密码。为了重复运行不受调用者 shell 污染，bin 可在只影响子进程的环境中显式移除 `OPENCODE_SERVER_PASSWORD`；不要修改用户全局环境。

`GET /global/health` 的 200 body 是：

```json
{ "healthy": true, "version": "1.18.18" }
```

两字段均 required，契约见 `#/paths/~1global~1health/get/responses/200`。可把它作为 ready 行之后的版本闸/证据，但 P0 仍需记录实际返回值。

## 3. Session create、prompt_async 与上下文复用

### 3.1 创建 session

请求：

```http
POST /session
x-opencode-directory: <同一个绝对目录，建议 percent-encoded>
content-type: application/json

{}
```

`POST /session` 的 body 是一个无 required 字段的 object；允许字段为 `parentID`、`title`、`agent`、`model`、`metadata`、`permission`、`workspaceID`（`#/paths/~1session/post/requestBody/content/application~1json/schema`）。响应是 HTTP 200（不是 201），body 为 `Session`。

必须从响应 JSON 的 `.id` 取得 `ses...`，不要从标题、slug 或 SSE 猜 ID。`Session.id`、`directory`、`title`、`version`、`time` 等契约见 `#/components/schemas/Session`（文件 `:15745`）。同时断言响应 `.directory` 等于临时目录，是 header 路由生效的直接证据。

### 3.2 异步 prompt

最小请求体：

```http
POST /session/{sessionID}/prompt_async
x-opencode-directory: <同一目录>
content-type: application/json

{
  "parts": [
    { "type": "text", "text": "..." }
  ]
}
```

`parts` 是唯一 required 顶层字段。可选字段还有 `messageID`、`model`（`providerID` + `modelID`）、`agent`、`noReply`、`tools`、`format`、`system`、`variant`。完整契约见 `#/paths/~1session~1{sessionID}~1prompt_async/post/requestBody/content/application~1json/schema`；文本 part 见 `#/components/schemas/TextPartInput`（文件 `:23065`），只要求 `type:"text"` 与 `text`。

成功是 HTTP 204、无 body。这个请求本身不会返回 assistant message；最终文本和 usage 必须从已建立的 SSE 流取得。规范只声明：

- 204 `Prompt accepted`；
- 400 `BadRequest | InvalidRequestError`；
- 404 `NotFoundError`。

特别地，`prompt_async` 的 OpenAPI 没有声明 409 / `SessionBusyError`。尖刺 3 因而不能只看第二次 HTTP 是否为 204；必须继续观察 SSE 与 message 时序。

### 3.3 上下文复用

第二轮复用的全部机制就是：

- URL 中继续使用第一次 `POST /session` 返回的同一 `sessionID`；
- 继续使用同一 `x-opencode-directory`；
- 等第一轮真正 idle 后再发第二个 `prompt_async`（尖刺 2 要测“顺序续上下文”，不要和尖刺 3 的并发混在一起）。

OpenCode 的 prompt loop 按 sessionID 读取消息历史；当前源码入口见 `/Volumes/Extreme SSD/Code/opencode/packages/opencode/src/session/prompt.ts:1052` 与 `:1081`。但“模型答对事实”仍是唯一验收证据。

建议第一轮要求“记住随机、高熵且不写入文件的事实，并只回复 ACK”，第二轮只问该事实。随机值应打印进证据，避免模型靠常识猜中。

## 4. SSE wire format、最终文本与 usage

### 4.1 连接顺序和帧格式

先连接 `GET /event`，带同一 directory，并等初始 `server.connected` 后再 create/prompt，避免非 replay 流的竞态。OpenAPI 返回 content type `text/event-stream`，data schema 为 `Event`（`#/paths/~1event/get/responses/200/content/text~1event-stream/schema`，`#/components/schemas/Event` 在文件 `:15260`）。

当前源码编码的 wire frame 是 SSE `event: message` + `data: JSON.stringify(event)`（`/Volumes/Extreme SSD/Code/opencode/packages/opencode/src/server/routes/instance/httpapi/handlers/event.ts:12`）。也就是说业务事件名在 JSON 的 `type` 字段，不在 SSE `event` 字段：

```text
event: message
data: {"id":"evt_...","type":"message.updated","properties":{...}}

```

1.18.18 bin 应把原始关键帧（可脱敏/截断正文但不能改事件名和错误）写入结果，验证实际 wire format。

### 4.2 需要解析的最小事件集

所有事件都先按 `properties.sessionID == sessionID` 过滤：

- `message.part.delta`：`properties` 包含 `sessionID/messageID/partID/field/delta`，见 `#/components/schemas/EventMessagePartDelta`（文件 `:35403`）。当 `field == "text"` 可增量拼接。
- `message.part.updated`：`properties.part` 是完整 `Part`；文本 part 的 `.type == "text"`、`.text` 是截至该事件的完整文本，见 `#/components/schemas/EventMessagePartUpdated`（文件 `:34035`）与 `#/components/schemas/TextPart`（文件 `:16392`）。以 `(messageID, partID)` 存最后值最稳妥，避免同时消费 delta 和 full update 造成重复。
- `message.updated`：`properties.info` 是 `Message`；当 `.role == "assistant"` 时是 `AssistantMessage`，见 `#/components/schemas/EventMessageUpdated`（文件 `:33976`）与 `#/components/schemas/AssistantMessage`（文件 `:16233`）。
- `session.status`：`properties.status.type` 是 `idle|retry|busy`，见 `#/components/schemas/EventSessionStatus`（文件 `:36281`）与 `#/components/schemas/SessionStatus`（文件 `:17266`）。
- `session.idle`：只带 `sessionID`，见 `#/components/schemas/EventSessionIdle`（文件 `:36310`）。
- `session.error`：保存完整 `properties.error`，不要只打印 Display 摘要。

### 4.3 usage 的确切位置

最终 usage 在 assistant `message.updated` 的 `properties.info.tokens`，不是 session idle 事件：

```json
{
  "role": "assistant",
  "time": { "created": 0, "completed": 0 },
  "tokens": {
    "total": 123,
    "input": 100,
    "output": 20,
    "reasoning": 3,
    "cache": { "read": 0, "write": 0 }
  },
  "cost": 0
}
```

`input/output/reasoning/cache.read/cache.write` required；`total` 可选，不能依赖它一定存在。以 `time.completed` 已出现且之后收到对应 session 的 `session.idle` 作为一轮完成边界；保存最后一个已 completed 的 assistant info 和属于该 messageID 的文本 parts。

`GET /session/{sessionID}/message`（OpenAPI `:6088`）可用于失败诊断和比对，但不能拿它冒充“从 SSE 收到最终文本与 usage”的验收证据。

## 5. 尖刺 3：忙碌 session 的观测设计

### 5.1 规范能提前回答什么

第二次 `prompt_async` 即使收到 204，也只说明 handler 接受了请求。由于规范没有 busy 响应，三种业务结果必须从 SSE/message 证据区分：

1. **被当前循环接住/steer**：第二条 user message 在第一轮尚 busy 时出现，且当前运行的后续 assistant/tool 行为明确引用它；记录 assistant `parentID`、message IDs 和时间线。
2. **错误**：记录第二 HTTP 的完整 status/body；若 HTTP 仍 204，则记录随后 `session.error.properties.error` 的完整 JSON 与原文。
3. **排队**：第二 user message 先持久化，但直到第一 assistant 完成/idle 后才有以第二 user message 为 parent 的新 assistant；记录两个完成边界和处理时刻。

“第二条 user message 被保存”本身不等于“它进入同一轮”。应至少打印：两个 POST 的发出/返回时间、全部 user/assistant message IDs、assistant `parentID`、`session.status`/`session.idle`、关键 tool part 和文本。

### 5.2 checkout 源码形成的假设（不是结论）

当前 checkout 的行为是：先创建/保存 user message，再进入 loop（`/Volumes/Extreme SSD/Code/opencode/packages/opencode/src/session/prompt.ts:1052`）；runner 已处于 `Running` 时，另一个 `ensureRunning` 调用等待现有 run 的同一个 Deferred，而不是直接抛 Busy（`/Volumes/Extreme SSD/Code/opencode/packages/opencode/src/effect/runner.ts:115`）。`prompt_async` 又把整个工作 fork 后立即返回 204，并把异步 cause 发布为 `session.error`（`/Volumes/Extreme SSD/Code/opencode/packages/opencode/src/server/routes/instance/httpapi/handlers/session.ts:311`）。

这使“HTTP 204 + 第二 user message 已保存 + 与现有 runner 汇合”成为较强预期，但仍没有回答第二内容是本次 provider 调用中被看见、当前循环下一步被看见、还是留到之后。必须用 1.18.18 实测时间线定性。

耗时任务建议让 Agent 调 bash 做可观测的约 8–12 秒延迟，并在收到 tool running / `session.status busy` 后立刻发第二 prompt；不要仅依赖固定 sleep 猜第一轮已经 busy。

## 6. 远程 MCP 的确切配置与 token

### 6.1 `opencode.json`

在临时 directory **第一次被 OpenCode 请求使用前**写入：

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "openwork": {
      "type": "remote",
      "url": "http://127.0.0.1:PORT/mcp",
      "enabled": true,
      "oauth": false,
      "headers": {
        "Authorization": "Bearer SPIKE_RANDOM_TOKEN"
      }
    }
  }
}
```

这是官方文档 remote 配置的完整形状（`/Volumes/Extreme SSD/Code/opencode/packages/web/src/content/docs/mcp-servers.mdx:130`）；schema 也明确 `type/url` required，`enabled/headers/oauth/timeout` optional（`/Volumes/Extreme SSD/Code/opencode/packages/core/src/v1/config/mcp.ts:44`）。`oauth:false` 不是 token header 所必需，但能关闭不相关的 OAuth auto-detection，让尖刺只验证静态 header。

项目配置会从当前 directory 向上查到 git root，见官方配置文档 `/Volumes/Extreme SSD/Code/opencode/packages/web/src/content/docs/config.mdx:109` 和 loader `/Volumes/Extreme SSD/Code/opencode/packages/opencode/src/config/config.ts:406`。MCP state 初始化时读取 config 并连接（`/Volumes/Extreme SSD/Code/opencode/packages/opencode/src/mcp/index.ts:492`），所以在已创建该 directory instance 后才写配置，可能不会触发本次连接；先写文件、起 MCP listener，再对 directory 发 OpenCode 请求。

### 6.2 OpenCode transport/header 行为

OpenCode remote MCP 客户端会：

1. 先尝试单端点 Streamable HTTP；
2. 失败后尝试 legacy SSE；
3. 两种 transport 都把 `mcp.headers` 作为 request headers 发送。

源码见 `/Volumes/Extreme SSD/Code/opencode/packages/opencode/src/mcp/index.ts:236`。官方 OpenCode 测试逐请求断言 `Authorization: Bearer test-token` 和自定义 header，见 `/Volumes/Extreme SSD/Code/opencode/packages/opencode/test/mcp/headers.test.ts:41`。

MCP config key 与 tool name 会被拼成模型侧工具名：`openwork` + `echo` => `openwork_echo`；规则见 `/Volumes/Extreme SSD/Code/opencode/packages/opencode/src/mcp/catalog.ts:117`。

可先用 `GET /mcp`（同一 directory）诊断期望的 `{ "openwork": { "status": "connected" } }`，但 P0 的通过证据必须同时具备：

- MCP server 收到 initialize/tools-list 以及 tools-call；
- tools-call 的工具确为 `echo`，参数与结果能在 OpenCode `message.part.updated` tool part/最终文本中对应；
- server 对每个相关 HTTP 请求实际读取到预设 token。不要只证明“配置文件里写了 token”。

### 6.3 Rust server 库选择

截至 2026-08-18，首选官方 Model Context Protocol Rust SDK `rmcp = 3.1.3`：

- 版本 3.1.3 于 2026-08-17 发布、未 yank，MSRV 1.88；本机 Rust 1.89 满足；来源是官方 `modelcontextprotocol/rust-sdk`。[crates.io 一手元数据](https://crates.io/api/v1/crates/rmcp)、[固定 release](https://github.com/modelcontextprotocol/rust-sdk/releases/tag/rmcp-v3.1.3)、[固定版本 workspace](https://github.com/modelcontextprotocol/rust-sdk/blob/rmcp-v3.1.3/Cargo.toml)。
- 它直接提供单端点 Streamable HTTP server，正好匹配 OpenCode 首选 transport；不需要手写 MCP framing，也不需要实现旧式 GET-SSE/POST 双端点。[固定版本 transport 文档](https://github.com/modelcontextprotocol/rust-sdk/blob/rmcp-v3.1.3/README.md#transports)。
- 本 workspace 现有依赖没有 MCP crate；已有 serde/tokio 可沿用，但 server 还实际需要 `rmcp` 和 HTTP service（官方例子使用 axum 0.8）。

最小依赖方向：

```toml
rmcp = { version = "=3.1.3", default-features = false, features = [
  "server",
  "macros",
  "transport-streamable-http-server",
] }
axum = { version = "0.8", default-features = false, features = ["http1", "tokio"] }
```

最小 server 使用 `StreamableHttpService::new(factory, LocalSessionManager, config)`，再 `Router::nest_service("/mcp", service)`；参考[官方 Streamable HTTP server](https://github.com/modelcontextprotocol/rust-sdk/blob/rmcp-v3.1.3/examples/servers/src/counter_streamhttp.rs)与[官方工具实现](https://github.com/modelcontextprotocol/rust-sdk/blob/rmcp-v3.1.3/examples/servers/src/common/counter.rs)。token 校验应在 `/mcp` 前的 axum middleware 做：不匹配直接 401，同时记录实际 header；官方有同形的 [bearer-token 示例](https://github.com/modelcontextprotocol/rust-sdk/blob/rmcp-v3.1.3/examples/servers/src/simple_auth_streamhttp.rs)。

其他候选如 `rust-mcp-sdk`、`pmcp` 不是协议官方 SDK，本尖刺没有理由越过维护活跃且直接支持所需 transport 的 `rmcp`。

## 7. 审批事件、回复与 reject feedback

### 7.1 默认 external_directory 的触发条件

默认 permission 是 `"*":"allow"`，但 `external_directory:"*":"ask"`，并放行 OpenCode 自身 tmp/tool-output、skills/reference 等目录；`.env` read 另有 ask（`/Volumes/Extreme SSD/Code/opencode/packages/opencode/src/agent/agent.ts:108`）。外部路径判断允许 active directory 与正常 git worktree 内的路径；非 git 的 worktree `/` 特意不视为“所有路径都在 worktree 内”（`/Volumes/Extreme SSD/Code/opencode/packages/opencode/src/project/instance-context.ts:13`）。

尖刺应把工作目录与目标文件做成两个 sibling 临时目录，并让模型调用 Read 读取目标绝对路径。不要把目标放在 OpenCode 自身 `${os.tmpdir()}/opencode/*` 白名单下。外部 read 会为目标父目录形成 `.../*` pattern，事件 metadata 含 filepath/parentDir（`/Volumes/Extreme SSD/Code/opencode/packages/opencode/src/tool/external-directory.ts:15`）。

### 7.2 `permission.asked`

`GET /event` 上 legacy 事件 JSON：

```json
{
  "id": "evt_...",
  "type": "permission.asked",
  "properties": {
    "id": "per...",
    "sessionID": "ses...",
    "permission": "external_directory",
    "patterns": ["/outside/parent/*"],
    "metadata": { "filepath": "/outside/parent/file" },
    "always": ["/outside/parent/*"],
    "tool": { "messageID": "msg...", "callID": "..." }
  }
}
```

确切契约见 `#/components/schemas/EventPermissionAsked`（文件 `:36048`）和 `#/components/schemas/PermissionRequest`（文件 `:22820`）。注意有两个不同 ID：

- 外层 `evt_...` 是事件 ID；
- `properties.id` 的 `per...` 才是 reply URL 的 `requestID`。

只接受匹配本次 `sessionID` 且 `permission == "external_directory"` 的请求，避免误批同 directory 其他请求。

### 7.3 reply once / reject

`once`：

```http
POST /permission/{properties.id}/reply
x-opencode-directory: <同一目录>
content-type: application/json

{ "reply": "once" }
```

`reject` + message：

```json
{
  "reply": "reject",
  "message": "SPIKE_REJECTION_MARKER: do not read that file"
}
```

`reply` 的 enum 仅 `once|always|reject`，`message` optional；成功是 HTTP 200、JSON `true`。契约见 `#/paths/~1permission~1{requestID}~1reply/post`。不存在返回 204 的约定。

源码明确把带 message 的 reject 转成 `PermissionCorrectedError { feedback }`（`/Volumes/Extreme SSD/Code/opencode/packages/opencode/src/permission/index.ts:109`），其模型可见错误文本为：

```text
The user rejected permission to use this specific tool call with the following feedback: <message>
```

定义见 `/Volumes/Extreme SSD/Code/opencode/packages/core/src/v1/permission.ts:13`。所以第二次触发 ask 时使用高熵 marker，reject 后等待模型继续，并在后续 tool error/assistant 文本中找 marker，是“message 能被模型看到”的强证据。第一次 `once` 不会加入 approved rules（同 permission 源码 `:142`），因此可以在同一 session 下一轮再次读取同一路径来触发 reject；仍需实测确认事件确实再次出现。

## 8. 五个 bin 的最小证据清单

### Spike 1 — drive

- `opencode --version`、ready URL、`GET /global/health` 原始 JSON；
- SSE `server.connected`；
- `POST /session {}` 的 200 body（至少 id/directory/version）；
- `POST /session/{id}/prompt_async` 的 204；
- 同 session 的最终 text part、completed assistant `tokens`、idle 事件；
- 明确写实际用通的是 `/session/*`，并记录若探测 `/api/session/*` 的结果；不得调用 `/experimental/*`。

### Spike 2 — context

- create response 中 `sessionID`；
- 两次 prompt URL 都含同一 ID，同一 directory；
- 第一轮 idle 后第二轮才发；
- 随机事实、第二轮问题和模型实际答案。

### Spike 3 — busy（最重要）

- 第一 prompt 开始、tool running/busy、第二 prompt 发出与两个 HTTP 响应的单调时钟时间线；
- 两个 user IDs、所有 assistant IDs/parentIDs、idle/error 原始事件；
- 若错误，完整 status/body 或 `session.error.properties.error` 原文；
- 若排队，明确第二轮何时开始；若同轮接住，指出哪条事件/文本证明第二内容真被当前循环看到。

### Spike 4 — MCP

- 实际写入的完整 `opencode.json`（token 值可部分遮蔽，但 server 日志与配置须能关联）；
- `/mcp` connected 仅作诊断；
- server 收到 initialize/list/call，`echo` 参数/结果；
- 每个相关请求实际收到的 Authorization 是否匹配；
- OpenCode 侧 tool part 名 `openwork_echo` 和最终回答。

### Spike 5 — permission

- 外部文件与 active directory 的绝对路径；
- `permission.asked` 完整关键 properties，尤其 requestID/sessionID/permission/pattern/metadata；
- `once` reply request/HTTP 200 true，之后 read/assistant 继续；
- 新 ask 的 `reject` + 高熵 message request/HTTP 200 true；
- 后续事件或 assistant 输出中 marker 出现的原始片段；
- 若没有 ask，不要加自定义 rules 伪造通过，应检查目标是否落在 directory/worktree/默认白名单并如实报告。

## 9. 实现时容易踩的坑

1. `/session/{id}/event` 不存在；P0 用 `/event` + sessionID filter。
2. `/event` 必须带 directory；无 header 会连到 server cwd instance。
3. SSE 是非 durable live stream；先等 `server.connected` 再发 prompt。
4. SSE 的业务名是 data JSON `.type`；不要只看 SSE `event`（通常只是 `message`）。
5. `prompt_async` 204 不是完成，也不是忙碌行为结论。
6. 最终 text 在 part 事件、usage 在 assistant message 事件、完成边界在 idle；三者要关联同一 session/message。
7. `tokens.total` optional；记录 required 的分项。
8. permission event 外层 event ID 不能拿去 reply；要用 `properties.id`。
9. `GET /permission` 是当前 directory instance 中所有 session 的 pending，不是跨所有 Agent directory 的单次全局查询。
10. `opencode.json` 必须在该 directory instance 首次加载前写好；MCP listener 也必须先 bind 出端口。
11. MCP token 要由 server 实际验证/记录，不能只检查静态 JSON。
12. reject feedback 的模型可见性要靠高熵 marker 实测，不能只凭 HTTP 200。
13. 本地 checkout 与安装版相差四个 patch；源码只能指导观测，最终结论只认 1.18.18 的原始证据。
