# Agent Note: 最小内核加插件的项目结构

Status: proposed

## 问题

`openwork-core` 承担了几乎所有能力，扩展点靠硬编码。证据（2026-10-03 的 `dev`）：

- `core.rs` 有 1,620 行生产代码、40 个 pub 方法，还兼做子 Agent 宿主（`impl SubAgentHost`）。`inspect_context_window` 又重新组装了一遍 run loop 的请求。
- 新增一个模型 Provider 要改约 7 个文件：`models/provider/driver.rs` 的 3 个 `match`、`factory.rs`、`adapters/openai_chat/request.rs`、`core/provider.rs`、`core.rs` 与前端 `providerContracts.ts`。没有注册表。
- 新增一个工具要改 Rust 与前端约 13 个文件：`agent/src/definition.rs` 的工具名列表、`toolActivity.ts` 的白名单、`traceToolIcons.tsx` 的两张表，以及按工具名分支的组件。
- World State section 在 `context/world_state/mod.rs` 中有 6 处具名字段与 `match`。
- `EngineRegistry` 存在，但 `computer/process.rs` 只登记 OpenCode，前端把 `engineId` 写成字面量 `'opencode'`。
- 数据库 Record 类型直接返回给 Desktop（`desktop/src-tauri/src/commands/runtime.rs`），违反 [architecture.md](../../../../docs/architecture.md) §1。
- 前端的 Rust DTO 是手写镜像（`desktop/src/bridge/compat.ts`），没有生成，也没有对拍测试。

pi 与 DSH 的结构更清晰，原因相同：

- 内核极小：pi 的 `packages/agent` 不到 2,600 行，只认消息、工具、事件。
- 扩展只有一个入口：pi 用 `ExtensionAPI` 加约 40 个类型化事件，DSH 用 Cordis 的服务与事件。
- 依赖单向，内核不带界面。

## 提议

继续使用 Rust（用户决定，2026-10-03）。Rust 没有稳定的运行时插件 ABI，所以插件分两层：

- **编译期插件**是主体（用户决定，2026-10-04）。每个能力是一个 crate，在启动时向内核的注册表与钩子登记。
- 每个 crate 有一份 README，按 [crate README 模板](../../../../docs/templates/crate-readme.md) 写。模板照搬 DSH 的包 README。
- **进程外扩展**给第三方，用 MCP 或 stdin/stdout JSON-RPC。现在不做，等有需求再定。

### 目标结构

```text
crates/
  kernel/         会话事件模型、Agent Loop、钩子、注册表、取消。不做 IO
  llm/            Responses 协议的模型调用；厂商与模型是配置数据（见 responses-only-model-access 提议）
  sandbox/        sandbox（trait）+ sandbox-seatbelt
  tool-fs/        read、write、edit、grep、glob、list
  tool-bash/      bash 与危险命令检测
  permission/     审批与越界，挂在 before_tool_call 钩子上
  context-*/      每个 World State section 一个插件：agents-md、skills、sandbox-policy
  compaction/     修剪与摘要
  plan/  skills/  subagent/   各自独立的插件
  session-store/  会话事件日志与派生索引（见 session-event-log 提议）
  trace/          订阅事件，写 Trace
  collab-server/  房间、消息、卡片等业务事实
  collab-computer/ Engine 进程与 Engine 插件
  host/           按 profile 装配插件：desktop、headless、collab-computer
apps/
  desktop/        Tauri，只经 host 的命令与事件访问
```

### 内核

内核只定义这几样东西：

- 会话事件与内存状态；
- Agent Loop：组装请求、调用模型、执行工具、结束 Turn；
- 钩子：按注册顺序调用，可以观察、改写或阻断；
- 注册表：Tool、Provider、ContextSource、Engine；
- 插件接口：`trait Plugin { fn id(&self) -> &'static str; fn install(&self, host: &mut HostBuilder) -> Result<(), PluginError>; }`。插件只能经 `HostBuilder` 登记。

初版钩子取自现有 run loop 的调用点。具体名称与组合语义在实现时定：

| 钩子 | 时机 | 能做什么 | 现在的占用者 |
|---|---|---|---|
| `context` | 组装请求前 | 追加或改写消息 | World State、计划提醒 |
| `before_model_request` | 发出请求前 | 改写请求 | 预算估算、Trace |
| `before_tool_call` | 执行工具前 | 放行、阻断、要求审批 | 权限、危险命令检测 |
| `after_tool_result` | 工具返回后 | 改写结果 | 结果上限与落盘 |
| `turn_end` | Turn 结束前 | 要求继续或结束 | 未收尾计划的观测 |
| `session_event` | 每个事件追加后 | 只观察 | 持久化、Trace、Desktop 推送 |

### 只为真实的可替换点抽接口

只有已经存在两个实现、或已经计划第二个实现时，才抽能力接口：`sandbox`、`engine`、`session-store`。`llm` 只有 Responses 一种协议，`plan`、`skills` 也只有一个实现，它们是普通插件，不抽接口。这与 CLAUDE.md 的“不加投机的抽象”一致，DSH 也有同样的规则。

### 前端

- 用 `ts-rs` 或 `specta` 从 Rust 生成 TS 类型，删除手写镜像，并加检查保证生成结果不过期。
- 工具在定义中声明展示方式。前端按声明渲染，不再按工具名分支。

### 协作

- 拆成 `collab-server` 与 `collab-computer` 两个 crate。
- Engine 是插件。`EngineRegistry` 按 profile 登记 Engine，不再在 `computer/process.rs` 写死 OpenCode；前端的 Engine 选项来自 Server 下发的清单。
- 协作暂时不使用 OpenWork 自己的内核（用户决定，2026-10-03）。协作与工作台仍是两套独立的 Session 概念。

### 迁移顺序

每一步结束时产品都能工作，`scripts/check.sh` 全部通过。每一步只搬完整的能力，不留新旧两条路径。

1. 建 `kernel` crate。把 run loop 的调用点改成钩子，现有逻辑原地登记为第一批钩子。
2. 把 World State section 搬成 `context-*` 插件。删除 `SystemContextBuilder` 中被忽略的参数。
3. 依次把权限、计划、技能、子 Agent、压缩、Trace 搬成插件。
4. 把 `openwork-tools` 拆成 `tool-fs` 与 `tool-bash`。模型调用改为只用 Responses 协议，厂商改为配置（见 [responses-only-model-access 提议](../simplification/2026-10-03-responses-only-model-access.md)）。
5. 建 `host` 与 profile。`core.rs` 拆完后删除 `openwork-core`。
6. 生成 TS 类型；工具声明展示方式。
7. 切换存储，见同日的 session-event-log 与 drop-postgres-and-redis 提议。它可以与第 5、6 步并行。
8. 协作拆成 `collab-server` 与 `collab-computer`，Engine 按 profile 登记。

## 考虑过的方案

**改用 TypeScript，像 DSH、pi 一样在运行时加载插件。** 没有采用：用户决定继续使用 Rust（2026-10-03）。Seatbelt 沙箱与 Tauri 都已在 Rust 上。

**照搬 Codex `ext/extension-api` 的 contributor 设计。** Codex 的核心之外，每个扩展是 `codex-rs/ext/` 下的一个 crate。`extension-api/src/contributors.rs` 为每种扩展点定义一个 trait，共 12 个，例如 `ContextContributor`、`ToolContributor`、`ToolLifecycleContributor`、`ApprovalReviewContributor`。`extension-api/src/registry.rs` 的 `ExtensionRegistryBuilder<C>` 为每种 trait 提供一个登记方法，`build()` 产生只读的 `ExtensionRegistry`。扩展 crate 导出 `install(&mut builder, deps)`，例如 `ext/goal/src/extension.rs` 的 `install_with_backend`；宿主在 `app-server/src/extensions.rs` 中逐个显式调用。`extension-api/notes.md` 把每个扩展归入几种 contributor，例如 goal 是 Tool + Runtime。本提议采用它的三点：编译期装配、每个扩展一个 crate、宿主显式调用 `install`。钩子是否也做成每种扩展点一个类型化 trait，在实现钩子时定，以 Codex 为参照。没有整体照搬：Codex 一次定义 12 种扩展点；本提议只从现有 run loop 的调用点取 6 个钩子，有插件需要时再加，不加投机的抽象。

**Rust 动态库插件（`dylib`、`abi_stable`）。** 没有采用：Rust 没有稳定的 ABI，要么退回 C ABI，要么维护 `abi_stable` 的类型约束。第三方扩展改走进程外协议，成本更低。

**WASM 插件。** 暂不采用：WASM 访问进程与文件受限，而现在没有第三方插件的需求。有需求时再评估。

**保留 `openwork-core`，只拆大文件。** 没有采用：拆文件解决不了扩展点硬编码与两套 Session 概念。

**协作把 OpenWork 自己的内核登记为内置 Engine，合并两套 Session 概念。** Raft 内置了自己的 Agent（`packages/daemon/src/drivers/index.ts` 的 `builtin`）。暂不采用（用户决定，2026-10-03）。

**一次重写。** 没有采用：违反 CLAUDE.md“分层增长系统”与“不要用能工作的产品换取未完成的复杂度”。

## 验收条件

- `kernel` 不依赖任何能力 crate（用 `cargo tree` 检查），也不依赖 IO crate。
- 新增一个工具：新建 crate，在 profile 中加一行；前端不改代码就能显示。
- 新增一个提供 Responses 接口的厂商：只改 `config.json`。
- 新增一个 World State section：新建插件，不改 `kernel`。
- `openwork-core` 删除；`desktop/src-tauri` 中不出现存储层的 Record 类型。
- TS 类型由生成器产生，过期时 `scripts/check.sh` 失败。
- 每个 crate 有 README，结构符合 crate README 模板。
- 每一步都同步更新受影响的子系统页与 Agent Note。

## 风险

- 多个插件改写同一个结果时的组合语义要先定规则。pi 由每个事件声明返回值语义，DSH 的 waterfall 要求监听者显式调用 `next()`。
- 钩子会让调用链变长，问题更难追。需要 Trace 记录每个钩子的决定。
- 工作量大，迁移期间新功能开发要暂停或放慢。
