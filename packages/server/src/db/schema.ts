import type { AgentId, MessageId, RoomId, UserId } from "@crew/protocol";
import { sql } from "drizzle-orm";
import { bigint, check, pgTable, primaryKey, text, timestamp, unique, uuid } from "drizzle-orm/pg-core";

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
 * 房间：私聊（`direct`，一个用户与一个 Agent）或群聊（`group`，有名字，Agent 成员可以增加）。
 * `next_seq` 是下一条消息的序号来源：写消息时锁住房间行并加一，同一房间内的序号因此连续、不跳号。
 */
export const rooms = pgTable(
  "rooms",
  {
    id: uuid("id").primaryKey().defaultRandom().$type<RoomId>(),
    kind: text("kind", { enum: ["direct", "group"] }).notNull(),
    /** 群聊的名字。私聊没有名字，界面显示 Agent 的名字。 */
    name: text("name"),
    /** 私聊双方的组合键，保证同一对用户与 Agent 只有一个私聊房间。 */
    directKey: text("direct_key").unique(),
    nextSeq: bigint("next_seq", { mode: "number" }).notNull().default(0),
    createdAt: createdAt(),
  },
  (t) => [
    check("rooms_kind_known", sql`${t.kind} IN ('direct', 'group')`),
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

/** 消息的作者是用户或 Agent 之一：两个可空外键，恰好一个有值。 */
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
    body: text("body").notNull(),
    createdAt: createdAt(),
  },
  (t) => [
    unique("messages_room_seq_unique").on(t.roomId, t.seq),
    check("messages_seq_positive", sql`${t.seq} > 0`),
    check("messages_one_author", sql`num_nonnulls(${t.authorUserId}, ${t.authorAgentId}) = 1`),
    check("messages_body_not_blank", sql`btrim(${t.body}) <> ''`),
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
