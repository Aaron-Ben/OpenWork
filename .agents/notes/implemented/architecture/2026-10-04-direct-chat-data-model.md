# Agent Note: 私聊的数据模型

Status: implemented

## 问题

第 2 步的最小私聊要保存用户、Agent、房间与消息，并让 Agent 只读到自己还没处理过的消息。表结构要从一开始就保证几件以后难改的事：消息的作者真实存在，房间内的序号连续，Agent 的已读位置可靠。

## 决策

表与约束见 [messaging.md](../../../../docs/subsystems/messaging.md) 第 2 节，定义在 `packages/server/src/db/schema.ts`。这里只记录其中的选择：

- 用户与 Agent 分成两张表。本机只有一个用户，由一行固定的记录表示。成员关系也分成 `room_users` 与 `room_agents`，做法来自 raft 的 `channel_humans` 与 `channel_agents`（`raft:packages/server/src/db/schema.ts`）。
- 消息的作者用两个可空外键加“恰好一个不为空”的约束，数据库保证作者真实存在。
- 消息序号按房间递增：写入时锁住房间行，`next_seq` 加一。同一房间内序号连续、不跳号，Agent 的已读位置才可靠。
- Agent 的已读位置单独存在 `agent_read_cursors`，做法来自 raft 的 `agent_channel_read_cursors`。
- ID 用 UUID，由 PostgreSQL 的 `gen_random_uuid()` 生成。代码里用 branded 类型区分（`packages/protocol/src/ids.ts`）。时间戳用 `timestamptz`。
- 迁移文件由 `drizzle-kit generate` 生成到 `packages/server/drizzle/`；electron-vite 构建时把它复制到主进程产物旁，主进程经环境变量 `CREW_MIGRATIONS_DIR` 告诉 Server 迁移目录，测试直接传入路径。

写入、已读位置与验收见 [messaging.md](../../../../docs/subsystems/messaging.md) 第 4、6、9 节。

## 考虑过的方案

**用户与 Agent 共用一张参与者表。** Rust 版与 cumora 这样做，消息与成员只需要一个外键。没有采用：用户与 Agent 的属性差别大，分表后各自的约束更清楚；外键完整性改由“两个可空外键”保证。

**消息作者用 `sender_type` 加 `sender_id`。** raft 的 `messages` 这样做，写起来简单。没有采用：`sender_id` 没有外键，类型与 ID 不匹配、作者不存在或已删除时数据库都不会拒绝。

**全局自增的消息序号。** raft 用 `bigserial`，不需要锁房间行。没有采用：并发写入时序号可能与提交顺序不一致，读者按“某序号之后”取消息会漏掉后提交的小序号，raft 为此另有 `raft:packages/sync-core` 检查缺号。

**Agent 的已读位置放在成员表上。** Rust 版的 `room_members.last_read_seq` 这样做，少一张表。没有采用：用户与 Agent 分表后成员表也分成两张，单独的已读表更自然，成员关系与每轮都在更新的已读位置也就分开了。

**由代码生成 UUID。** 插入前就知道 ID，便于乐观更新与重试去重。没有采用：现在还没有这类需求，数据库生成更简单；需要时只要把默认值换成代码里的一行。

## 后果

- 数据库直接拒绝不存在的作者、两个作者同时为空或同时存在、房间内重复的序号（`packages/server/test/db.test.ts`）。
- 同一房间的写入因为锁住房间行而排队，不同房间互不影响。
- 读消息时要连接 `users` 与 `agents` 两张表才能得到作者的显示名（`packages/server/src/messages.ts`）。
