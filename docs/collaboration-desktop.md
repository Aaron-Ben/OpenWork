# macOS 协作桌面端

协作模式是 OpenWork Desktop 中与工作台平级的第二个 Shell。React 负责投影 Server canonical state；Tauri host 负责监督本机 Collaboration Runtime 并把 typed Desktop command 转发给 Server。

业务语义见 [collaboration.md](collaboration.md)，存储约束见 [collaboration-data-model.md](collaboration-data-model.md)。

## 1. 产品边界

| Desktop 负责 | Desktop 不负责 |
|---|---|
| 启动、监督和停止 Server/Computer 子进程 | 不执行 Agent loop |
| 管理 Agent profile/runtime config | 不选择 triage 结果或 Agenda 候选 |
| 创建 Room、管理 Group audience、发送用户消息 | 不直接写 PostgreSQL/Redis |
| 管理 Board/Column 结构和删除 | 不向 Agent 暴露结构删除命令 |
| 展示 Runtime、Engine、Runner、Message、Run | 不持有 Agent JWT 或 Engine session |
| 把 Desktop SSE invalidation 转成 Tauri event | 不把 SSE 当成业务事实 |

只支持当前 Mac。界面没有远程机器、Computer 选择器或后台 Runtime 开关；Desktop 正常退出即停止 Collaboration Runtime。

## 2. Shell 与导航

`desktop/src/App.tsx` 根据 `modeStore` 选择工作台或 `CollabShell`：

```text
App
├── workbench → AppShell
└── collab    → CollabShell
```

mode 写入 `localStorage` 的 `openwork-mode`。切换 mode 只替换 React 组件树，不重启 Tauri host 或 Collaboration Runtime。

`CollabRail` 有三个顶层目的地：

1. Rooms；
2. Agents；
3. Boards。

macOS Rail 顶部保留窗口拖拽区，避免内容压在窗口控制按钮下。协作 feature 不 import 工作台 chat feature；两者只共享 UI primitive、主题、i18n 和通用错误处理。

## 3. Tauri supervisor

应用 setup 顺序：

```text
OpenWorkCore bootstrap
  → Core event bridge
  → CollabDaemonClient::discover_or_start
  → Server/Computer ready + first heartbeat
  → collaboration invalidation bridge
  → manage CollabDaemonClient + OpenWorkCore
```

`CollabDaemonClient` 的 interface 只有三类能力：

- `call(DesktopCommand)`；
- `subscribe_invalidations()`；
- `shutdown()`。

它内部持有：

- `runtime.lock` 文件句柄；
- 当前 HTTP connection 和 Desktop secret；
- supervisor command channel；
- Server/Computer child handles；
- Desktop SSE task；
- 当前 runtime 目录。

首次启动失败会让 Tauri setup 失败，不注册半可用 managed state。运行期任一子进程退出时，connection 暂时变为 unavailable；成组重启成功后，新 command 自动使用新 RuntimeSession connection。

应用事件循环返回后，`lib.rs` 先取出 setup 已注册的 `CollabDaemonClient`，再等待 `shutdown()` 完成，最后以原 exit code 结束 Desktop。shutdown 总等待上限为 30 秒；Computer 有 20 秒外层窗口，Server 有 5 秒窗口。

## 4. Tauri command seam

React bridge 位于 `desktop/src/bridge/collab.ts`；Rust adapter 位于 `desktop/src-tauri/src/commands/collab.rs`。两边只传 `openwork-collab::protocol::desktop` 定义的 typed command/result。

### 4.1 Runtime 与 Agent

```text
status
list_agents
create_agent
update_agent
set_agent_agenda
archive_agent
restore_agent
```

Agent editor 可修改 display name、role、persona、Engine、main model 和 triage model。当前 UI 的 Engine select 只有 `opencode`，但 protocol 的 `engine_id` 是强类型运行属性；第二个生产 adapter 可用后才增加选项。

Agent 卡片组合展示：

- profile/config；
- archive/active；
- last Engine inventory；
- current-session Engine readiness；
- Runner running/error 与 last error；
- Agenda enabled。

### 4.2 Room 与 Message

```text
list_rooms
create_direct_room
create_group_room
list_room_members
add_group_member
remove_group_member
send_message
list_messages
```

Desktop 用户可以创建 Group 并改变 Group audience。Direct Room 创建是“创建或返回已有 Room”。成员操作只接受 Agent ID；固定用户始终由 Server 管理。

### 4.3 Board、Column 与 Card

```text
list_boards
create_board
update_board
delete_board
create_board_column
update_board_column
move_board_column
delete_board_column
assign_card
delete_card
```

Board 页面提供完整结构管理：

- Board title/description；
- Column title、terminal 标记和顺序；
- Card assignee；
- 空容器删除约束的错误展示；
- 最近 Run 与 Card focus。

Agent 创建/更新/移动 Card 走另一套 Agent command interface，不经过这些 Desktop Tauri command。

## 5. SSE 到 WebView

Tauri host 独占 `/desktop/events`。收到 `InvalidationEvent` 后，通过固定事件名发给 WebView：

```text
openwork://collaboration-invalidation
```

payload：

```ts
interface CollabInvalidation {
  id: string
  kind: 'runtime_ready' | 'agent_config' | 'message' |
        'engine_inventory' | 'runner_status'
  subjectId: string | null
  revision: number | null
  publishedAt: number
}
```

event 只表示“某类 canonical view 可能变化”。前端不能把它直接拼进 store；收到后重新调用 Tauri command 获取完整 view。

`CollabShell` 当前处理：

- Runtime/Engine/Runner invalidation → 重新取 `status`；
- runtime-ready / agent-config → 同时重新取 Agent list；
- message invalidation 不直接改变 message store。

消息页面仍每 2 秒读取当前 Room Message/Run，Board 页面每 5 秒读取 Board/Run。这两个 poll 是 durable fallback，也避免 WebView 必须理解 Agent SSE 或 Redis wake。

Desktop SSE 断线后由 Tauri host 独立指数退避重连；WebView 不参与 credential 或 connection 管理。

## 6. Store 所有权

| store | 拥有 |
|---|---|
| `modeStore` | workbench/collab mode |
| `collabNavigationStore` | Rail view 与 active Room |
| `roomStore` | Room list 和创建流程 |
| `messageStore` | 当前 Room 的 Message、成员、Run 与 loading/error |
| `agentStore` | Agent list 和 profile/config 操作 |
| `boardStore` | Board tree、Run、结构操作与错误 |
| `runtimeStore` | RuntimeStatus snapshot |

Store 只保存 UI snapshot 和 request 状态。权限、幂等、顺序、terminal、archive、HELD、triage 与 Agenda 决策都由 Server 裁决。

## 7. Rooms 页面

```text
┌────────┬──────────────┬──────────────────────────────────┐
│ Rail   │ Room list    │ Message pane                     │
│        │ direct/group │ Message / members / Run status   │
│        │ create room  │ composer                         │
└────────┴──────────────┴──────────────────────────────────┘
```

Message pane 提供：

- 全量列出当前 Room Message；
- 用户文本发送；
- Group 成员查看、添加和移除；
- 依据最近 Run 展示 Agent running/failed/completed 状态。

当前 `list_messages` 没有分页参数，因此 Desktop 也没有游标窗口化；如果长期 Room 的数据量要求分页，必须先扩展 Server 的 typed read command，不能只在前端截断 canonical list。

## 8. Agents 页面

Agent Manager 分开 active 与 archived Agent：

- create 自动使用 Server 返回的 slug ID；
- update 提交完整 profile/runtime config；
- archive 立即停止 Runner但保留记录；
- restore 触发 config revision 变化和 reconcile；
- Agenda 开关默认关闭；
- runtime status 来自 Server 聚合的 inventory/readiness/Runner snapshot。

Persona 编辑器只编辑用户人格部分。Computer 写入最终 `AGENTS.md` 时还会追加代码拥有的协作契约，因此 persona 不能移除权限与交互规则。

## 9. Boards 页面

Board 页面将整个 workspace Board tree 作为一次 snapshot 读取。UI 提交语义位置：

- Column move 发送 `before_column_id` 或 append；
- Card move 由 Agent command 发送目标 Column 和 `before_card_id`；
- UI 不自行永久保存 position；
- Server 返回重排后的完整 Board。

删除冲突、跨 Board before target、非空 Column/Board 等错误通过统一 `CommandError` 显示，不由前端预判代替 Server 校验。

## 10. 错误与恢复

| 情况 | Desktop 表现 |
|---|---|
| 初始 Runtime 启动失败 | setup 失败，应用不进入半可用协作状态 |
| Runtime 正在成组替换 | command 返回 unavailable；store 保留旧 snapshot 并显示错误 |
| Server 业务拒绝 | 显示稳定 error code/message |
| SSE 断线 | Tauri host 重连；页面 poll 继续 |
| Runner error | Agent 卡片显示 last error，其他 Agent 不受影响 |
| Engine missing/error | inventory 与 current readiness 分开显示 |
| Desktop 正常退出 | 等待协作进程组停止后退出 |

## 11. 验收

1. setup 完成后再读取 managed collaboration state；
2. 指定无效 child ready metadata 时启动失败且 runtime 目录为空；
3. 忽略 SIGTERM 的 child 会在 deadline 后被强制结束；
4. Desktop SSE 有限响应流关闭后能重新连接并继续投影 invalidation；
5. 真实 supervisor 测试分别杀死 Server 和 Computer，两个 PID 与 RuntimeSession 都整体更换；
6. 每次替换后 fake OpenCode 仍能通过真实 shim 发布 durable reply；
7. 正常 shutdown 后没有协作 child，当前 runtime 目录为空；
8. PostgreSQL/Redis 在 Desktop shutdown 后仍可连接；
9. React bridge 参数与 Rust command DTO 一致；
10. Room/Agent/Board store 的 loading/error 不承载业务真相。
