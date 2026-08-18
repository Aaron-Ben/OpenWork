# OpenWork 协作模式 P0 技术尖刺报告

实测日期：2026-08-18

实测平台：macOS / `opencode 1.18.18` / Rust 1.89.0

API 权威来源：`/Volumes/Extreme SSD/Code/opencode/packages/sdk/openapi.json`（OpenAPI 3.1，162 个 path）

## 0. 总结

| 尖刺 | 结论 | 最关键证据 |
|---|---|---|
| 1. 驱动 OpenCode | **成立** | `POST /session` → `prompt_async` 204 → `/event` 收到文本、usage、idle |
| 2. 复用 session 上下文 | **成立** | 同一 `ses_...` 第二轮逐字答出第一轮事实 |
| 3. 忙碌 session 再发 prompt | **被运行中的循环接住** | 第二条 user message 在第一轮工具仍 running 时落入历史；无中间 idle，随后被同一 runner 的下一次 loop step 处理 |
| 4. remote MCP 回连 | **成立** | Agent 调用 `openwork_echo`；echo server 在该次工具 HTTP 请求中收到配置 token |
| 5. 审批收发 | **成立** | 收到 `permission.asked`；`once` 后继续读取；`reject + message` 进入 tool error，且模型逐字复述 message |

五项都在安装版 1.18.18 上实测通过。没有调用任何 `/experimental/` 端点。

同时发现两个必须由后续设计回写处理的 API 落差：

1. 权威 OpenAPI **没有**非实验的 `GET /session/{id}/event`。本尖刺实际使用的是同 directory 的 `GET /event`，客户端按 `properties.sessionID` 过滤。
2. `GET /event` 和 `GET /permission` 都受 `x-opencode-directory` 选择的 instance 约束，不是跨所有 Agent directory 的 server-global 视图；真正跨 instance 的事件端点是 `GET /global/event`。

本报告只记录结论，没有修改 `docs/` 下的设计文档。

## 1. 能不能驱动 OpenCode

### 结论

**成立。** 1.18.18 可由 Rust 子进程启动、从就绪行取得随机端口、在指定 directory 建 session、异步发 prompt，并从 SSE 得到最终文本和 token usage。

两套 path family 都存在：

- `GET /session?limit=1`：HTTP 200；
- `GET /api/session`：HTTP 200，响应是 v2 的 `{data, cursor}` 形状。

实际打通 P0 的是非实验 **`/session/*` v1**，因为权威规格只有它提供 `prompt_async`；v2 对应的是 `/api/session/{id}/prompt`，不是同一个接口。

### 实测证据

启动与版本：

```text
opencode server listening on http://127.0.0.1:4096
GET /global/health
200 {"healthy":true,"version":"1.18.18"}
```

建 session 的实际请求：

```http
POST /session
x-opencode-directory: /var/folders/.../T/.tmpdhx1UR
content-type: application/json

{"title":"OpenWork P0 spike 1"}
```

关键响应：

```json
{
  "id": "ses_feab22c48ffe6Ch1Y3W32JzCmc",
  "directory": "/private/var/folders/.../T/.tmpdhx1UR",
  "title": "OpenWork P0 spike 1",
  "version": "1.18.18"
}
```

`/var/folders/...` 与 `/private/var/folders/...` 是 macOS 同一路径的别名；响应说明 instance 的 cwd 确实是临时目录，不是 server 进程 cwd。

异步 prompt：

```http
POST /session/ses_feab22c48ffe6Ch1Y3W32JzCmc/prompt_async
x-opencode-directory: /var/folders/.../T/.tmpdhx1UR
content-type: application/json

{
  "parts": [
    {"type":"text","text":"Reply with exactly P0_DRIVE_OK and no other text."}
  ]
}
```

响应是 `204 No Content`，body 为空。随后 `GET /event` 的关键事件依次包括：

```json
{"type":"session.status","properties":{"sessionID":"ses_feab...","status":{"type":"busy"}}}
{"type":"message.part.updated","properties":{"part":{"type":"text","text":"P0_DRIVE_OK"}}}
{
  "type": "message.updated",
  "properties": {
    "info": {
      "id": "msg_0154dd4ae001CTrGTZiMQtbAul",
      "role": "assistant",
      "finish": "stop",
      "time": {"created":1787063817390,"completed":1787063823312},
      "tokens": {
        "input":20348,
        "output":7,
        "reasoning":18,
        "cache":{"read":1664,"write":0},
        "total":22037
      }
    }
  }
}
{"type":"session.status","properties":{"sessionID":"ses_feab...","status":{"type":"idle"}}}
```

最终提取结果：

```text
turn.final_text=P0_DRIVE_OK
turn.usage={"cache":{"read":1664,"write":0},"input":20348,"output":7,"reasoning":18,"total":22037}
SPIKE1_RESULT=成立
```

### 踩坑与绕过

- `prompt_async` 自身不返回 message，只返回 204；文本在 `message.part.updated`，usage 在 completed assistant 的 `message.updated.properties.info.tokens`，idle 只是完成边界。
- `GET /session/{id}/event` 不在权威 spec 中；使用 `GET /event`，带相同 directory header，并按 session id 过滤。
- `opencode serve --port 0` 不能猜端口，必须解析 stdout 的 listening line。
- OpenCode 需要读用户 provider 登录态并写自己的日志；在受限 sandbox 内直接启动会报 `FileSystem.open (.../opencode.log)`，正常主机权限下可运行。

### 对设计的影响

- “一个 server + 每请求 `x-opencode-directory`”的地基成立。
- 设计中的单 session SSE 路径需要回写为实际可用的 `/event + sessionID filter`，或明确整体迁移到另一套 v2 API；当前不能混搭。

## 2. 同一 session 能不能续上下文

### 结论

**成立。** session id 直接来自 `POST /session` 响应的 `.id`；第二轮在 URL 中复用同一 id，并继续使用同一 directory header。

### 实测证据

创建响应给出：

```text
session.id.from=POST /session response field `id`
session.id=ses_feab16bfbffei4tUhnw7zOoO35
```

第一轮请求：

```json
{
  "parts": [{
    "type":"text",
    "text":"Remember this exact fact for my next message: P0_CONTEXT_FACT_7Q9M2. Reply with exactly STORED."
  }]
}
```

第一轮最终文本是 `STORED`，收到 idle 后再发第二轮：

```http
POST /session/ses_feab16bfbffei4tUhnw7zOoO35/prompt_async

{"parts":[{"type":"text","text":"What exact fact did I ask you to remember? Reply with the fact only, byte for byte."}]}
```

第二轮仍是 HTTP 204，SSE 最终文本：

```text
round2.final_text=P0_CONTEXT_FACT_7Q9M2
round2.usage={"cache":{"read":21760,"write":0},"input":563,"output":12,"reasoning":35,"total":22370}
SPIKE2_RESULT=成立
```

第二轮 assistant 的关键关联：

```json
{
  "id":"msg_0154eb34a001ZIPBs0yhESkSza",
  "role":"assistant",
  "parentID":"msg_0154eb346001G38Ev9Nj45NmQf",
  "sessionID":"ses_feab16bfbffei4tUhnw7zOoO35",
  "time":{"created":1787063874378,"completed":1787063877935}
}
```

### 踩坑与绕过

- 第一轮结束时 `session.status=idle` 后还可能紧跟一个旧式 `session.idle` 事件；它可能在第二个 POST 后才被客户端读到。collector 不能把“看到任意 idle”当作新一轮结束，必须先观察到本轮 busy/user/assistant，再接受 idle。
- 上下文复用需要同时保持 session id 和 directory；只复用 id、却把请求路由到另一个 directory instance，不是有效复用。

### 对设计的影响

`opencode_session_id` 足以承载长期上下文引用；P1 不需要自己实现消息历史、context window 或 compaction 链路。

## 3. 向忙碌的 session 发 prompt 会怎样

### 结论

**被运行中的循环接住。** 更精确地说：第二条内容不会进入已经在进行中的那一次 provider/tool 调用；它在第一轮 tool 仍运行时立刻写成同 session 的新 user message，随后由**同一个尚未 idle 的 runner 的下一次 loop step**处理。两轮之间没有外部排队边界，也没有 HTTP/SSE 错误。

第一轮要求最终回答 `FIRST_DONE`，但第二条 prompt 在 tool 运行期间进入后，第一 assistant 以 `finish:"tool-calls"` 完成；runner 随即以第二条 user message 为 parent 创建新 assistant，最终只输出 `SECOND_DONE`。这证明第二内容确实改变了当前运行循环的后续处理，而不是仅被静默存储。

### 实测证据

第一轮请求返回：

```text
round1.prompt_async.status=204 No Content
```

确认“真的忙碌”后才发第二轮：

```json
{
  "type":"message.part.updated",
  "properties":{
    "part":{
      "messageID":"msg_0154fa143001WfGvq9nFWvFTkA",
      "tool":"bash",
      "state":{
        "input":{"command":"sleep 10"},
        "status":"running",
        "time":{"start":1787063940938}
      }
    }
  }
}
```

此时第二次实际请求：

```http
POST /session/ses_feab05f71ffeCDNtv3AxE3vLUu/prompt_async

{"parts":[{"type":"text","text":"This is SECOND_PROMPT. Reply with exactly SECOND_DONE when you process it."}]}
```

完整 HTTP 结果（本次没有错误原文）：

```text
status: 204 No Content
headers: content-length: 0; vary: Origin
body: <empty>
```

关键单调时间线：

| 时刻（事件内 Unix ms） | 证据 |
|---:|---|
| `1787063940938` | 第一 assistant 的 `bash sleep 10` 进入 `running` |
| `1787063940954` | 第二 user message `msg_0154fb75a0012okYdgIIJJBPfj` 已创建 |
| `1787063951005` | 第一轮 bash 才完成；第二 user 比它早约 10 秒进入 session |
| `1787063951023` | 第一 assistant `msg_0154fa143001WfGvq9nFWvFTkA` completed，`finish:"tool-calls"` |
| `1787063951025` | 新 assistant `msg_0154fdeb10016dKmc180NTjAUW` 创建，`parentID` 正是第二 user message |
| `1787063953932` | 新 assistant completed，文本 `SECOND_DONE` |
| 最后 | 第一次出现 idle；两条 prompt 之间没有 idle |

程序对原始事件序列的索引结果：

```text
busy.sequence.second_user_event_index=Some(8)
busy.sequence.first_assistant_completed_index=Some(19)
busy.sequence.idle_event_indices=[36]
SPIKE3_OUTCOME=被运行中的循环接住
```

`GET /session/{id}/message` 在 idle 后返回四条消息，关系为：

```text
user 1:      msg_0154fa138001ev3VQRq01nd14b
assistant 1: msg_0154fa143001WfGvq9nFWvFTkA parent=user 1, finish=tool-calls
user 2:      msg_0154fb75a0012okYdgIIJJBPfj
assistant 2: msg_0154fdeb10016dKmc180NTjAUW parent=user 2, text=SECOND_DONE
```

### 踩坑与绕过

- 仅看到 `session.status=busy` 不够强；bin 等到具体 bash tool part 为 `running` 后才发第二条。
- 仅看到第二个 HTTP 204 也不能分类；必须同时比较 user/assistant message id、parentID、tool 时间与 idle 边界。
- `prompt_async` 即使后台失败也可能先返回 204；实现同时保留 `session.error` 原始 JSON。此次没有收到 `session.error`。

### 对设计的影响

- 单个 busy session 不需要 daemon 先等 idle 再投递一次 prompt；OpenCode 当前 runner 能接住它。
- 但它是“下一 loop step 读取最新 user message”，不是修改已发出的 provider request。第二 prompt 可能让第一 prompt 原本预期的 tool 后续文本（本次的 `FIRST_DONE`）不再产生。后续调度应把这视为 steer/追加输入语义，不应承诺两个独立完整 turn 都会各自结束。
- 本尖刺只证明一次 busy 注入；不能外推为无限并发 prompt 都有稳定顺序，P1 仍应保留单 Agent 串行化和可观测时序。

## 4. MCP 能不能连回来

### 结论

**成立。** 使用官方 `rmcp 3.1.3` 的 Streamable HTTP server，OpenCode 能发现、调用 echo 工具；配置的 token 出现在实际 tools/call HTTP 请求中。

### 实测配置

临时 `<home>/opencode.json` 的确切内容：

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "openwork": {
      "enabled": true,
      "headers": {
        "X-OpenWork-Token": "p0-agent-token-4K8D"
      },
      "oauth": false,
      "timeout": 10000,
      "type": "remote",
      "url": "http://127.0.0.1:64089/mcp"
    }
  }
}
```

`openwork` 是 MCP config key，server tool 名为 `echo`，所以 Agent 侧工具名是 `openwork_echo`。

### 实测证据

prompt：

```json
{
  "parts": [{
    "type":"text",
    "text":"Call the openwork_echo tool exactly once with text P0_MCP_ECHO_2H7X..."
  }]
}
```

OpenCode SSE 显示工具可见并被调用：

```json
{
  "type":"message.part.updated",
  "properties":{
    "part":{
      "tool":"openwork_echo",
      "state":{
        "input":{"text":"P0_MCP_ECHO_2H7X"},
        "status":"running"
      }
    }
  }
}
```

随后同一 tool part 完成：

```json
{
  "tool":"openwork_echo",
  "state":{
    "input":{"text":"P0_MCP_ECHO_2H7X"},
    "output":"P0_MCP_ECHO_2H7X",
    "status":"completed"
  }
}
```

rmcp handler 从该次 HTTP request 的 headers 读取到：

```text
mcp.tool.invocation=ToolInvocation {
  text: "P0_MCP_ECHO_2H7X",
  token: Some("p0-agent-token-4K8D")
}
mcp.tool.visible_in_agent_event=true
turn.final_text=P0_MCP_ECHO_2H7X
SPIKE4_RESULT=成立
```

### 踩坑与绕过

- `opencode.json` 和 MCP listener 必须在 OpenCode 首次加载该 directory instance 前准备好；否则本次 instance 可能看不到新配置。
- 设置 `oauth:false`，避免静态 token 尖刺被 OAuth auto-discovery 干扰。
- 不能只证明 JSON 里“写过 token”；bin 在工具处理器内读取实际 HTTP request parts，验证 tools/call 真携带它。
- `rmcp` 的 Streamable HTTP service 仍需要 HTTP listener/router；本尖刺用 axum 0.8，只实现一个 echo 工具，没有手写 MCP framing。

### 对设计的影响

远程 MCP 作为 daemon 动作接口的地基成立；每 Agent 在 `opencode.json` 配不同 header token，可由 MCP server 在真实工具请求上识别身份。正式实现仍需把当前演示 marker 换成生成、存储和校验的专属凭证。

## 5. 审批能不能收到并回复

### 结论

**成立。** 默认权限确实对 directory 外文件触发 `external_directory` ask；`once` 后 tool 继续；`reject` 的 message 同时出现在 tool error 和模型最终文本中。

### 实测证据：once

工作目录与目标文件是两个 sibling 临时目录：

```text
home.directory=/var/folders/.../T/.tmpVtARyl
external.once_file=/var/folders/.../T/.tmpt4jlc2/once.txt
external.reject_file=/var/folders/.../T/.tmpt4jlc2/reject.txt
```

第一次收到：

```json
{
  "id":"evt_01551b86a002o4c0vzbqV0IfVn",
  "type":"permission.asked",
  "properties":{
    "id":"per_01551b86a001rP6SxzYIdrhjiN",
    "sessionID":"ses_feaae5d79ffet5pHZGAL4C3UvV",
    "permission":"external_directory",
    "patterns":["/var/folders/.../T/.tmpt4jlc2/*"],
    "metadata":{
      "filepath":"/var/folders/.../T/.tmpt4jlc2/once.txt",
      "parentDir":"/var/folders/.../T/.tmpt4jlc2"
    },
    "tool":{
      "callID":"chatcmpl-tool-45080794e137412c84903f442c0393d2",
      "messageID":"msg_01551a3350014p5jG9kC66utbc"
    }
  }
}
```

reply URL 使用的是 `properties.id` 的 `per_...`，不是外层 `evt_...`：

```http
POST /permission/per_01551b86a001rP6SxzYIdrhjiN/reply

{"reply":"once"}
```

响应和继续执行证据：

```text
once.permission.reply.status=200 OK
once.permission.reply.body=true
permission.replied.properties.reply=once
read tool state.status=completed
read tool output contains P0_EXTERNAL_ONCE_9F3L
once.turn.final_text=P0_EXTERNAL_ONCE_9F3L
```

### 实测证据：reject + message

`once` 没有永久放行相同外部父目录；下一轮读取 sibling 文件再次收到新的 `permission.asked`：

```text
properties.id=per_01551d1b3001B6z9fKjG1EAyc6
properties.permission=external_directory
properties.metadata.filepath=.../.tmpt4jlc2/reject.txt
```

实际 reply：

```http
POST /permission/per_01551d1b3001B6z9fKjG1EAyc6/reply

{
  "reply":"reject",
  "message":"P0_REJECT_REASON_DO_NOT_READ"
}
```

响应：

```text
reject.permission.reply.status=200 OK
reject.permission.reply.body=true
```

随后 read tool 的原始错误消息：

```text
The user rejected permission to use this specific tool call with the following feedback: P0_REJECT_REASON_DO_NOT_READ
```

模型下一 step 的最终文本：

```text
reject.turn.final_text=P0_REJECT_REASON_DO_NOT_READ
SPIKE5_RESULT=成立
```

最终文本不含被拒文件的内容 `P0_EXTERNAL_REJECT_6W2R`。

### 踩坑与绕过

- permission event 有两个 id；reply 必须用 `properties.id` 的 `per_...`。
- 外部文件不能落入 active directory、worktree 或 OpenCode 自身临时白名单；使用两个 sibling `tempdir` 可稳定触发默认 ask。
- `reject` 的 HTTP 200 只说明回复成功，不足以证明模型看见 message；本尖刺同时检查 tool error 原文和模型最终输出中的高熵 marker。
- 与普通 turn 相同，permission reply 后还要继续读 SSE 直到 completed assistant + idle，不能把 `permission.replied` 当作 turn 完成。

### 对设计的影响

- OpenCode 默认权限层和审批协议可直接复用，不需要自定义 ruleset。
- reject message 的模型反馈通路成立。
- 设计中的“全局待审批角标”不能靠不带 directory 的单次 `GET /permission` 覆盖所有 Agent；后续实现必须按 Agent directory 聚合，或基于 `/global/event` 维护 server-wide 待决集合。具体方案应在设计回写时决定。

## 6. 可重复运行与代码范围

新 crate：`crates/openwork-collab`

```text
src/lib.rs                 OpenCode 子进程、HTTP、SSE 和证据收集
src/bin/spike1_drive.rs    驱动与 usage
src/bin/spike2_context.rs  session 上下文复用
src/bin/spike3_busy.rs     busy prompt 时序
src/bin/spike4_mcp.rs      rmcp echo + token
src/bin/spike5_permission.rs  once / reject 审批
README.md                  前置条件与运行命令
API-RESEARCH.md            OpenAPI/源码/crates.io 一手资料调查笔记
```

运行：

```sh
cargo run -p openwork-collab --bin spike1_drive
cargo run -p openwork-collab --bin spike2_context
cargo run -p openwork-collab --bin spike3_busy
cargo run -p openwork-collab --bin spike4_mcp
cargo run -p openwork-collab --bin spike5_permission
```

依赖仅限尖刺所需：现有 workspace 的 reqwest/serde/serde_json/tokio，加 `tempfile`、axum，以及固定版本的官方 `rmcp = 3.1.3`。没有依赖 `openwork-core`，没有 migrations、表、业务逻辑或 desktop 变更，也没有修改现有五个 crate。
