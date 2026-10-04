import type { AgentId, MessageId, RoomId, UserId } from "@crew/protocol";
import { sql } from "drizzle-orm";
import { bigint, check, pgTable, primaryKey, text, timestamp, unique, uuid } from "drizzle-orm/pg-core";

// 表结构的取舍见设计 Agent Note 的“第 2 步的实现决策 → 数据模型”。

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
    persona: text("persona").notNull(),
    engineId: text("engine_id").notNull(),
    model: text("model").notNull(),
    createdAt: createdAt(),
  },
  (t) => [
    check("agents_display_name_not_blank", sql`btrim(${t.displayName}) <> ''`),
    check("agents_persona_not_blank", sql`btrim(${t.persona}) <> ''`),
  ],
);

/**
 * 房间。`next_seq` 是下一条消息的序号来源：写消息时锁住房间行并加一，
 * 同一房间内的序号因此连续、不跳号。
 */
export const rooms = pgTable(
  "rooms",
  {
    id: uuid("id").primaryKey().defaultRandom().$type<RoomId>(),
    kind: text("kind", { enum: ["direct"] }).notNull(),
    /** 私聊双方的组合键，保证同一对用户与 Agent 只有一个私聊房间。 */
    directKey: text("direct_key").unique(),
    nextSeq: bigint("next_seq", { mode: "number" }).notNull().default(0),
    createdAt: createdAt(),
  },
  (t) => [
    check("rooms_kind_known", sql`${t.kind} IN ('direct')`),
    check("rooms_direct_has_key", sql`${t.kind} <> 'direct' OR ${t.directKey} IS NOT NULL`),
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

/** Agent 在每个房间处理到的序号。Turn 成功后才推进，失败时下次重新处理。 */
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
  },
  (t) => [
    primaryKey({ columns: [t.agentId, t.roomId] }),
    check("agent_read_cursors_seq_non_negative", sql`${t.lastReadSeq} >= 0`),
  ],
);
