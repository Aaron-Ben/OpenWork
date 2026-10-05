import type { AgentId, MessageId, RoomId, UserId } from "@crew/protocol";
import { sql } from "drizzle-orm";
import type { AnyPgColumn } from "drizzle-orm/pg-core";
import {
  bigint,
  check,
  doublePrecision,
  integer,
  jsonb,
  pgTable,
  primaryKey,
  text,
  timestamp,
  unique,
  uniqueIndex,
  uuid,
} from "drizzle-orm/pg-core";

// 表结构的取舍见 Agent Note：私聊的数据模型（2026-10-04-direct-chat-data-model），群聊（2026-10-05-group-chat）。

const createdAt = () => timestamp("created_at", { withTimezone: true }).notNull().defaultNow();

/** 本机只有一个用户，由 `ensureLocalUser` 保证恰好一行。 */
export const users = pgTable("users", {
  id: uuid("id").primaryKey().defaultRandom().$type<UserId>(),
  displayName: text("display_name").notNull(),
  createdAt: createdAt(),
});

export const agents = pgTable(
  "agents",
  {
    id: uuid("id").primaryKey().defaultRandom().$type<AgentId>(),
    displayName: text("display_name").notNull(),
    /** 在消息里点名用的 `@handle`。格式由 `agents_handle_format` 约束，与 protocol 的 `Handle` 一致。 */
    handle: text("handle").notNull().unique(),
    persona: text("persona").notNull(),
    engineId: text("engine_id").notNull(),
    model: text("model").notNull(),
    createdAt: createdAt(),
  },
  (t) => [
    check("agents_display_name_not_blank", sql`btrim(${t.displayName}) <> ''`),
    check("agents_persona_not_blank", sql`btrim(${t.persona}) <> ''`),
    check("agents_handle_format", sql`${t.handle} ~ '^[a-z0-9][a-z0-9-]{0,31}$'`),
  ],
);

/**
 * 房间：私聊（`direct`，一个用户与一个 Agent）、群聊（`group`，有名字，Agent 成员可以增加），
 * 或群聊里一条消息下的讨论串（`thread`）。
 * `next_seq` 是下一条消息的序号来源：写消息时锁住房间行并加一，同一房间内的序号因此连续、不跳号。
 *
 * 讨论串没有自己的成员行，谁能发言看父房间的成员；它的关注者是在讨论串里有已读位置的 Agent。
 * 取舍见 Agent Note：讨论串（2026-10-05-threads）。
 */
export const rooms = pgTable(
  "rooms",
  {
    id: uuid("id").primaryKey().defaultRandom().$type<RoomId>(),
    kind: text("kind", { enum: ["direct", "group", "thread"] }).notNull(),
    /** 群聊的名字。私聊没有名字，界面显示 Agent 的名字。 */
    name: text("name"),
    /** 私聊双方的组合键，保证同一对用户与 Agent 只有一个私聊房间。 */
    directKey: text("direct_key").unique(),
    nextSeq: bigint("next_seq", { mode: "number" }).notNull().default(0),
    /** 讨论串所在的群聊。 */
    parentRoomId: uuid("parent_room_id")
      .references((): AnyPgColumn => rooms.id, { onDelete: "cascade" })
      .$type<RoomId>(),
    /** 讨论串挂在哪条消息下。每条消息最多一个讨论串。 */
    parentMessageId: uuid("parent_message_id")
      .unique()
      .references((): AnyPgColumn => messages.id, { onDelete: "cascade" })
      .$type<MessageId>(),
    createdAt: createdAt(),
  },
  (t) => [
    check("rooms_kind_known", sql`${t.kind} IN ('direct', 'group', 'thread')`),
    check(
      "rooms_thread_has_parent",
      sql`(${t.kind} = 'thread') = (${t.parentRoomId} IS NOT NULL) AND (${t.kind} = 'thread') = (${t.parentMessageId} IS NOT NULL)`,
    ),
    check("rooms_direct_has_key", sql`${t.kind} <> 'direct' OR ${t.directKey} IS NOT NULL`),
    check("rooms_group_has_name", sql`${t.kind} <> 'group' OR btrim(coalesce(${t.name}, '')) <> ''`),
  ],
);

export const roomUsers = pgTable(
  "room_users",
  {
    roomId: uuid("room_id")
      .notNull()
      .references(() => rooms.id, { onDelete: "cascade" })
      .$type<RoomId>(),
    userId: uuid("user_id")
      .notNull()
      .references(() => users.id, { onDelete: "restrict" })
      .$type<UserId>(),
  },
  (t) => [primaryKey({ columns: [t.roomId, t.userId] })],
);

export const roomAgents = pgTable(
  "room_agents",
  {
    roomId: uuid("room_id")
      .notNull()
      .references(() => rooms.id, { onDelete: "cascade" })
      .$type<RoomId>(),
    agentId: uuid("agent_id")
      .notNull()
      .references(() => agents.id, { onDelete: "restrict" })
      .$type<AgentId>(),
  },
  (t) => [primaryKey({ columns: [t.roomId, t.agentId] })],
);

/**
 * 消息的作者是用户或 Agent 之一：两个可空外键，恰好一个有值。
 * `kind` 区分聊天（`text`）与通知（`system`，例如“领取了 #3”）。通知的作者是做这件事的人：
 * HELD、未读与唤醒都按作者算，做这件事的人不会被自己的通知拦下，也不把它算作未读。
 */
export const messages = pgTable(
  "messages",
  {
    id: uuid("id").primaryKey().defaultRandom().$type<MessageId>(),
    roomId: uuid("room_id")
      .notNull()
      .references(() => rooms.id, { onDelete: "cascade" })
      .$type<RoomId>(),
    seq: bigint("seq", { mode: "number" }).notNull(),
    authorUserId: uuid("author_user_id")
      .references(() => users.id, { onDelete: "restrict" })
      .$type<UserId>(),
    authorAgentId: uuid("author_agent_id")
      .references(() => agents.id, { onDelete: "restrict" })
      .$type<AgentId>(),
    kind: text("kind", { enum: ["text", "system"] })
      .notNull()
      .default("text"),
    body: text("body").notNull(),
    /** Agent 的消息是在哪一轮里发出的；用户的消息为空。 */
    runId: uuid("run_id").references((): AnyPgColumn => runs.id, { onDelete: "set null" }),
    /** 这条消息发出前，同一轮里在这个房间被 HELD 拦下的次数。 */
    heldBefore: integer("held_before").notNull().default(0),
    createdAt: createdAt(),
  },
  (t) => [
    unique("messages_room_seq_unique").on(t.roomId, t.seq),
    check("messages_seq_positive", sql`${t.seq} > 0`),
    check("messages_one_author", sql`num_nonnulls(${t.authorUserId}, ${t.authorAgentId}) = 1`),
    check("messages_body_not_blank", sql`btrim(${t.body}) <> ''`),
    check("messages_kind_known", sql`${t.kind} IN ('text', 'system')`),
  ],
);

/**
 * 任务：房间里一条消息变成的待办，编号在房间内递增，状态固定。负责人只能是 Agent。
 * 每个任务都有宿主消息：新建任务就是发一条正文为标题的消息再把它变成任务。
 * 并发靠带条件的更新，历史就是讨论串（私聊是时间线）里的通知。取舍见 Agent Note：任务（2026-10-05-tasks）。
 */
export const tasks = pgTable(
  "tasks",
  {
    id: uuid("id").primaryKey().defaultRandom(),
    roomId: uuid("room_id")
      .notNull()
      .references(() => rooms.id, { onDelete: "cascade" })
      .$type<RoomId>(),
    number: integer("number").notNull(),
    title: text("title").notNull(),
    status: text("status", { enum: ["todo", "in_progress", "in_review", "done", "closed"] })
      .notNull()
      .default("todo"),
    assigneeAgentId: uuid("assignee_agent_id")
      .references(() => agents.id, { onDelete: "restrict" })
      .$type<AgentId>(),
    createdByUserId: uuid("created_by_user_id")
      .references(() => users.id, { onDelete: "restrict" })
      .$type<UserId>(),
    createdByAgentId: uuid("created_by_agent_id")
      .references(() => agents.id, { onDelete: "restrict" })
      .$type<AgentId>(),
    messageId: uuid("message_id")
      .notNull()
      .unique()
      .references(() => messages.id, { onDelete: "cascade" })
      .$type<MessageId>(),
    claimedAt: timestamp("claimed_at", { withTimezone: true }),
    completedAt: timestamp("completed_at", { withTimezone: true }),
    createdAt: createdAt(),
    updatedAt: timestamp("updated_at", { withTimezone: true }).notNull().defaultNow(),
  },
  (t) => [
    unique("tasks_room_number_unique").on(t.roomId, t.number),
    check("tasks_number_positive", sql`${t.number} > 0`),
    check("tasks_title_not_blank", sql`btrim(${t.title}) <> ''`),
    check("tasks_status_known", sql`${t.status} IN ('todo', 'in_progress', 'in_review', 'done', 'closed')`),
    check("tasks_one_creator", sql`num_nonnulls(${t.createdByUserId}, ${t.createdByAgentId}) = 1`),
    // 进行中与待审一定有负责人：领取或分配之后才能开始。
    check(
      "tasks_working_has_assignee",
      sql`${t.status} NOT IN ('in_progress', 'in_review') OR ${t.assigneeAgentId} IS NOT NULL`,
    ),
  ],
);

/**
 * Agent 在每个房间的两个位置：
 *
 * - `delivered_seq`：已经交给 Agent 看过的最后一条。Computer 读取 inbox、HELD 返回新消息时推进。
 *   Agent 回复时，这之后别人发的消息会让回复被拦下（HELD）。
 * - `last_read_seq`：已经处理完的最后一条。Turn 成功后推进到 `delivered_seq`；失败时不动，下次重新处理。
 *
 * 加入群聊时两者都从当时的最新序号开始，新成员看不到加入前的消息。
 */
export const agentReadCursors = pgTable(
  "agent_read_cursors",
  {
    agentId: uuid("agent_id")
      .notNull()
      .references(() => agents.id, { onDelete: "cascade" })
      .$type<AgentId>(),
    roomId: uuid("room_id")
      .notNull()
      .references(() => rooms.id, { onDelete: "cascade" })
      .$type<RoomId>(),
    lastReadSeq: bigint("last_read_seq", { mode: "number" }).notNull().default(0),
    deliveredSeq: bigint("delivered_seq", { mode: "number" }).notNull().default(0),
  },
  (t) => [
    primaryKey({ columns: [t.agentId, t.roomId] }),
    check("agent_read_cursors_seq_non_negative", sql`${t.lastReadSeq} >= 0`),
    check("agent_read_cursors_delivered_not_behind", sql`${t.deliveredSeq} >= ${t.lastReadSeq}`),
  ],
);

/** 消息 @ 到的 Agent。只记录写入时是房间成员的 Agent；代码里的 `@` 不算。 */
export const messageMentions = pgTable(
  "message_mentions",
  {
    messageId: uuid("message_id")
      .notNull()
      .references(() => messages.id, { onDelete: "cascade" })
      .$type<MessageId>(),
    agentId: uuid("agent_id")
      .notNull()
      .references(() => agents.id, { onDelete: "cascade" })
      .$type<AgentId>(),
  },
  (t) => [primaryKey({ columns: [t.messageId, t.agentId] })],
);

/**
 * 用户在每个房间读到的序号，用来计算未读数。界面打开房间、看到新消息时推进，只前进。
 * 加入房间时从当时的最新序号开始。
 */
export const userReadCursors = pgTable(
  "user_read_cursors",
  {
    userId: uuid("user_id")
      .notNull()
      .references(() => users.id, { onDelete: "cascade" })
      .$type<UserId>(),
    roomId: uuid("room_id")
      .notNull()
      .references(() => rooms.id, { onDelete: "cascade" })
      .$type<RoomId>(),
    lastReadSeq: bigint("last_read_seq", { mode: "number" }).notNull().default(0),
  },
  (t) => [
    primaryKey({ columns: [t.userId, t.roomId] }),
    check("user_read_cursors_seq_non_negative", sql`${t.lastReadSeq} >= 0`),
  ],
);

/**
 * 运行记录：Agent 被唤醒后跑的一轮。Computer 开始时登记、结束时写结果；用量由每一步的事件累加。
 * Agent 的状态由它推出：有 `running` 的一轮就是回复中，最近一轮 `failed` 就是出错。
 */
export const runs = pgTable(
  "runs",
  {
    id: uuid("id").primaryKey().defaultRandom(),
    agentId: uuid("agent_id")
      .notNull()
      .references(() => agents.id, { onDelete: "cascade" })
      .$type<AgentId>(),
    outcome: text("outcome", { enum: ["running", "succeeded", "failed", "cancelled", "interrupted"] })
      .notNull()
      .default("running"),
    error: text("error"),
    /** 这一轮交给 Engine 的完整输入。 */
    prompt: text("prompt").notNull(),
    startedAt: timestamp("started_at", { withTimezone: true }).notNull().defaultNow(),
    endedAt: timestamp("ended_at", { withTimezone: true }),
    inputTokens: bigint("input_tokens", { mode: "number" }).notNull().default(0),
    outputTokens: bigint("output_tokens", { mode: "number" }).notNull().default(0),
    reasoningTokens: bigint("reasoning_tokens", { mode: "number" }).notNull().default(0),
    cacheReadTokens: bigint("cache_read_tokens", { mode: "number" }).notNull().default(0),
    cacheWriteTokens: bigint("cache_write_tokens", { mode: "number" }).notNull().default(0),
    cost: doublePrecision("cost").notNull().default(0),
    steps: integer("steps").notNull().default(0),
    replies: integer("replies").notNull().default(0),
    holds: integer("holds").notNull().default(0),
    /** 下一个事件的序号来源，与 `rooms.next_seq` 同样的做法。 */
    lastEventSeq: integer("last_event_seq").notNull().default(0),
  },
  (t) => [
    check("runs_outcome_known", sql`${t.outcome} IN ('running', 'succeeded', 'failed', 'cancelled', 'interrupted')`),
    check("runs_failed_has_error", sql`${t.outcome} <> 'failed' OR ${t.error} IS NOT NULL`),
    check("runs_ended_unless_running", sql`(${t.outcome} = 'running') = (${t.endedAt} IS NULL)`),
    // 每个 Agent 同一时间最多一轮在跑：Computer 的 Runner 是串行的，这里在数据库层保证。
    uniqueIndex("runs_one_running_per_agent").on(t.agentId).where(sql`${t.outcome} = 'running'`),
  ],
);

/** 一轮被哪个房间的哪几条消息唤醒。 */
export const runTriggers = pgTable(
  "run_triggers",
  {
    runId: uuid("run_id")
      .notNull()
      .references(() => runs.id, { onDelete: "cascade" }),
    roomId: uuid("room_id")
      .notNull()
      .references(() => rooms.id, { onDelete: "cascade" })
      .$type<RoomId>(),
    fromSeq: bigint("from_seq", { mode: "number" }).notNull(),
    toSeq: bigint("to_seq", { mode: "number" }).notNull(),
  },
  (t) => [
    primaryKey({ columns: [t.runId, t.roomId] }),
    check("run_triggers_range", sql`${t.fromSeq} >= 1 AND ${t.toSeq} >= ${t.fromSeq}`),
  ],
);

/** 一轮里的每一步。`data` 是 protocol 的 RunEvent 去掉 `seq`、`kind` 与 `at` 后的字段。 */
export const runEvents = pgTable(
  "run_events",
  {
    runId: uuid("run_id")
      .notNull()
      .references(() => runs.id, { onDelete: "cascade" }),
    seq: integer("seq").notNull(),
    kind: text("kind", { enum: ["step", "tool", "text", "step_end", "reply", "held"] }).notNull(),
    at: timestamp("at", { withTimezone: true }).notNull(),
    data: jsonb("data").notNull(),
  },
  (t) => [primaryKey({ columns: [t.runId, t.seq] })],
);
