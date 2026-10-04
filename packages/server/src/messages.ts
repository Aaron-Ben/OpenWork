import type { AgentId, MessageId, RoomId, UserId } from "@crew/protocol";
import { and, asc, eq, gt, ne, sql } from "drizzle-orm";
import type { Database } from "./db";
import { agentReadCursors, agents, messages, roomAgents, rooms, roomUsers, users } from "./db/schema";
import { RequestError } from "./errors";

export type Author = { kind: "user"; id: UserId } | { kind: "agent"; id: AgentId };

export interface MessageView {
  id: MessageId;
  roomId: RoomId;
  seq: number;
  author: { kind: "user" | "agent"; id: string; displayName: string };
  body: string;
  createdAt: Date;
}

export interface AppendResult {
  message: { id: MessageId; roomId: RoomId; seq: number };
  /** 需要唤醒的 Agent：房间里除作者以外的全部 Agent 成员。 */
  wakeAgentIds: AgentId[];
}

/**
 * 写入一条消息。用户与 Agent 都经过这里。
 *
 * 在一个事务里：确认作者是房间成员，把房间的 `next_seq` 加一取得序号，写入消息。
 * `UPDATE` 锁住房间行直到提交，同一房间的写入因此排队，序号连续且与提交顺序一致。
 * 调用方在事务提交后再发通知。
 */
export async function appendMessage(db: Database, roomId: RoomId, author: Author, body: string): Promise<AppendResult> {
  return db.transaction(async (tx) => {
    await assertMember(tx, roomId, author);

    const [room] = await tx
      .update(rooms)
      .set({ nextSeq: sql`${rooms.nextSeq} + 1` })
      .where(eq(rooms.id, roomId))
      .returning({ seq: rooms.nextSeq });
    if (!room) throw new RequestError(404, "房间不存在");

    const [message] = await tx
      .insert(messages)
      .values({
        roomId,
        seq: room.seq,
        body,
        authorUserId: author.kind === "user" ? author.id : null,
        authorAgentId: author.kind === "agent" ? author.id : null,
      })
      .returning({ id: messages.id, roomId: messages.roomId, seq: messages.seq });
    if (!message) throw new Error("写入消息失败");

    const wake = await tx
      .select({ agentId: roomAgents.agentId })
      .from(roomAgents)
      .where(
        author.kind === "agent"
          ? and(eq(roomAgents.roomId, roomId), ne(roomAgents.agentId, author.id))
          : eq(roomAgents.roomId, roomId),
      );

    return { message, wakeAgentIds: wake.map((row) => row.agentId) };
  });
}

type Transaction = Parameters<Parameters<Database["transaction"]>[0]>[0];

async function assertMember(tx: Transaction, roomId: RoomId, author: Author): Promise<void> {
  const [room] = await tx.select({ id: rooms.id }).from(rooms).where(eq(rooms.id, roomId));
  if (!room) throw new RequestError(404, "房间不存在");

  const member =
    author.kind === "user"
      ? await tx
          .select({ id: roomUsers.userId })
          .from(roomUsers)
          .where(and(eq(roomUsers.roomId, roomId), eq(roomUsers.userId, author.id)))
      : await tx
          .select({ id: roomAgents.agentId })
          .from(roomAgents)
          .where(and(eq(roomAgents.roomId, roomId), eq(roomAgents.agentId, author.id)));
  if (member.length === 0) throw new RequestError(403, "不是这个房间的成员");
}

/** 查询消息时附带作者的类型与显示名。 */
const messageColumns = {
  id: messages.id,
  roomId: messages.roomId,
  seq: messages.seq,
  body: messages.body,
  createdAt: messages.createdAt,
  authorUserId: messages.authorUserId,
  authorAgentId: messages.authorAgentId,
  userName: users.displayName,
  agentName: agents.displayName,
};

type MessageRow = {
  id: MessageId;
  roomId: RoomId;
  seq: number;
  body: string;
  createdAt: Date;
  authorUserId: UserId | null;
  authorAgentId: AgentId | null;
  userName: string | null;
  agentName: string | null;
};

function toView(row: MessageRow): MessageView {
  const author = row.authorUserId
    ? { kind: "user" as const, id: row.authorUserId, displayName: row.userName ?? "" }
    : { kind: "agent" as const, id: row.authorAgentId ?? "", displayName: row.agentName ?? "" };
  return { id: row.id, roomId: row.roomId, seq: row.seq, author, body: row.body, createdAt: row.createdAt };
}

/** 房间里的全部消息，按序号排列。 */
export async function listMessages(db: Database, roomId: RoomId): Promise<MessageView[]> {
  const [room] = await db.select({ id: rooms.id }).from(rooms).where(eq(rooms.id, roomId));
  if (!room) throw new RequestError(404, "房间不存在");

  const rows = await db
    .select(messageColumns)
    .from(messages)
    .leftJoin(users, eq(users.id, messages.authorUserId))
    .leftJoin(agents, eq(agents.id, messages.authorAgentId))
    .where(eq(messages.roomId, roomId))
    .orderBy(asc(messages.seq));
  return rows.map(toView);
}

export interface InboxRoom {
  roomId: RoomId;
  kind: "direct";
  messages: MessageView[];
}

/** Agent 在各房间已读位置之后的全部消息。没有未读消息的房间不出现。 */
export async function readInbox(db: Database, agentId: AgentId): Promise<InboxRoom[]> {
  const rows = await db
    .select({ ...messageColumns, kind: rooms.kind })
    .from(agentReadCursors)
    .innerJoin(rooms, eq(rooms.id, agentReadCursors.roomId))
    .innerJoin(
      messages,
      and(eq(messages.roomId, agentReadCursors.roomId), gt(messages.seq, agentReadCursors.lastReadSeq)),
    )
    .leftJoin(users, eq(users.id, messages.authorUserId))
    .leftJoin(agents, eq(agents.id, messages.authorAgentId))
    .where(eq(agentReadCursors.agentId, agentId))
    .orderBy(asc(messages.roomId), asc(messages.seq));

  const byRoom = new Map<RoomId, InboxRoom>();
  for (const row of rows) {
    const room = byRoom.get(row.roomId) ?? { roomId: row.roomId, kind: row.kind, messages: [] };
    room.messages.push(toView(row));
    byRoom.set(row.roomId, room);
  }
  return [...byRoom.values()];
}

/**
 * 把 Agent 在一个房间的已读位置推进到 `seq`。已读位置只前进：`seq` 小于当前位置时不变。
 *
 * @throws RequestError 404：Agent 不在这个房间；400：`seq` 超出房间已有的序号。
 */
export async function acknowledge(db: Database, agentId: AgentId, roomId: RoomId, seq: number): Promise<void> {
  await db.transaction(async (tx) => {
    const [cursor] = await tx
      .select({ nextSeq: rooms.nextSeq })
      .from(agentReadCursors)
      .innerJoin(rooms, eq(rooms.id, agentReadCursors.roomId))
      .where(and(eq(agentReadCursors.agentId, agentId), eq(agentReadCursors.roomId, roomId)));
    if (!cursor) throw new RequestError(404, "Agent 不在这个房间");
    if (seq > cursor.nextSeq) throw new RequestError(400, `序号 ${seq} 超出房间已有的消息`);

    await tx
      .update(agentReadCursors)
      .set({ lastReadSeq: sql`GREATEST(${agentReadCursors.lastReadSeq}, ${seq})` })
      .where(and(eq(agentReadCursors.agentId, agentId), eq(agentReadCursors.roomId, roomId)));
  });
}
