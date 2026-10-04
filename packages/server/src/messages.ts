import type { AgentId, InboxRoom, MessageId, MessageView, Participant, RoomId, RoomKind, UserId } from "@crew/protocol";
import { and, asc, desc, eq, gt, inArray, isNull, lt, ne, or, sql } from "drizzle-orm";
import type { Database } from "./db";
import { agentReadCursors, agents, messageMentions, messages, roomAgents, rooms, roomUsers, users } from "./db/schema";
import { RequestError } from "./errors";
import { mentionedHandles } from "./mentions";

export type Author = { kind: "user"; id: UserId } | { kind: "agent"; id: AgentId };

/** HELD 时一次最多返回的新消息条数。其余的留到下一次 HELD，或下一轮 Turn。 */
export const HELD_SHOWN_MAX = 20;

/** 不指定条数时，界面读取房间得到的消息数。 */
export const MESSAGE_PAGE_DEFAULT = 200;

export type PostResult =
  | {
      kind: "posted";
      message: { id: MessageId; roomId: RoomId; seq: number };
      /** 需要唤醒的 Agent，规则见 `wakeTargets`。 */
      wakeAgentIds: AgentId[];
    }
  | {
      /** Agent 的回复被拦下：房间里有它还没看到的、别人发的消息。消息没有写入。 */
      kind: "held";
      newMessages: MessageView[];
      omitted: number;
    };

type Transaction = Parameters<Parameters<Database["transaction"]>[0]>[0];

/**
 * 写入一条消息。用户与 Agent 都经过这里。
 *
 * 在一个事务里：锁住房间行，确认作者是房间成员；作者是 Agent 时先做 HELD 检查；
 * 然后把房间的 `next_seq` 加一取得序号，写入消息与它 @ 到的 Agent。
 * 锁住房间行使同一房间的写入排队，序号连续且与提交顺序一致，HELD 检查也不会与新消息交错。
 * 调用方在事务提交后再发通知。
 */
export async function postMessage(db: Database, roomId: RoomId, author: Author, body: string): Promise<PostResult> {
  return db.transaction(async (tx) => {
    const [room] = await tx.select({ nextSeq: rooms.nextSeq }).from(rooms).where(eq(rooms.id, roomId)).for("update");
    if (!room) throw new RequestError(404, "房间不存在");
    await assertMember(tx, roomId, author);

    if (author.kind === "agent") {
      const held = await heldMessages(tx, roomId, author.id, room.nextSeq);
      if (held) return held;
    }

    const seq = room.nextSeq + 1;
    await tx.update(rooms).set({ nextSeq: seq }).where(eq(rooms.id, roomId));
    const [message] = await tx
      .insert(messages)
      .values({
        roomId,
        seq,
        body,
        authorUserId: author.kind === "user" ? author.id : null,
        authorAgentId: author.kind === "agent" ? author.id : null,
      })
      .returning({ id: messages.id, roomId: messages.roomId, seq: messages.seq });
    if (!message) throw new Error("写入消息失败");

    const members = await tx
      .select({ id: agents.id, handle: agents.handle })
      .from(roomAgents)
      .innerJoin(agents, eq(agents.id, roomAgents.agentId))
      .where(eq(roomAgents.roomId, roomId));
    const handles = new Set(mentionedHandles(body));
    const mentioned = members.filter((member) => handles.has(member.handle)).map((member) => member.id);
    if (mentioned.length > 0) {
      await tx.insert(messageMentions).values(mentioned.map((agentId) => ({ messageId: message.id, agentId })));
    }

    if (author.kind === "agent") {
      // 通过了 HELD 检查，这之前的消息不是看过的就是它自己发的。
      await tx
        .update(agentReadCursors)
        .set({ deliveredSeq: seq })
        .where(and(eq(agentReadCursors.agentId, author.id), eq(agentReadCursors.roomId, roomId)));
    }

    return {
      kind: "posted",
      message,
      wakeAgentIds: wakeTargets(
        author,
        members.map((member) => member.id),
        mentioned,
      ),
    };
  });
}

/**
 * 一条消息唤醒哪些 Agent：用户的消息唤醒房间里全部 Agent 成员；
 * Agent 的消息只唤醒它 @ 到的其他成员，Agent 之间没人点名时不会来回接话。
 */
export function wakeTargets(author: Author, memberIds: AgentId[], mentionedIds: AgentId[]): AgentId[] {
  if (author.kind === "user") return memberIds;
  return mentionedIds.filter((id) => id !== author.id);
}

/**
 * HELD 检查：房间里有已投递位置之后、别人发的消息时，从最早的开始返回至多 `HELD_SHOWN_MAX` 条，
 * 已投递位置只推进到返回的最后一条：没有返回的消息仍然算没看过，Agent 再次回复时接着返回，
 * Turn 结束后它们也仍是未读，不会因为确认已读而被跳过。没有新消息时返回 undefined。
 */
async function heldMessages(
  tx: Transaction,
  roomId: RoomId,
  agentId: AgentId,
  latestSeq: number,
): Promise<PostResult | undefined> {
  const [cursor] = await tx
    .select({ deliveredSeq: agentReadCursors.deliveredSeq })
    .from(agentReadCursors)
    .where(and(eq(agentReadCursors.agentId, agentId), eq(agentReadCursors.roomId, roomId)));
  if (!cursor) throw new Error(`Agent ${agentId} 在房间 ${roomId} 没有已读记录`);

  const unseen = and(
    eq(messages.roomId, roomId),
    gt(messages.seq, cursor.deliveredSeq),
    or(isNull(messages.authorAgentId), ne(messages.authorAgentId, agentId)),
  );
  const [count] = await tx.select({ n: sql<number>`count(*)::int` }).from(messages).where(unseen);
  const total = count?.n ?? 0;
  if (total === 0) return undefined;

  const rows = await selectMessages(tx).where(unseen).orderBy(asc(messages.seq)).limit(HELD_SHOWN_MAX);
  const omitted = total - rows.length;
  // 全部返回了：推进到最新，之间夹着的只有它自己的消息。
  const delivered = omitted === 0 ? latestSeq : (rows.at(-1)?.seq ?? latestSeq);
  await tx
    .update(agentReadCursors)
    .set({ deliveredSeq: delivered })
    .where(and(eq(agentReadCursors.agentId, agentId), eq(agentReadCursors.roomId, roomId)));
  return { kind: "held", newMessages: rows.map(toView), omitted };
}

async function assertMember(tx: Transaction, roomId: RoomId, author: Author): Promise<void> {
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

/** 查询消息时附带作者的类型、显示名与 handle。 */
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
  agentHandle: agents.handle,
};

function selectMessages(db: Database | Transaction) {
  return db
    .select(messageColumns)
    .from(messages)
    .leftJoin(users, eq(users.id, messages.authorUserId))
    .leftJoin(agents, eq(agents.id, messages.authorAgentId))
    .$dynamic();
}

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
  agentHandle: string | null;
};

function toView(row: MessageRow): MessageView {
  const author: Participant = row.authorUserId
    ? { kind: "user", id: row.authorUserId, displayName: row.userName ?? "", handle: null }
    : { kind: "agent", id: row.authorAgentId ?? "", displayName: row.agentName ?? "", handle: row.agentHandle };
  return { id: row.id, seq: row.seq, author, body: row.body, createdAt: row.createdAt.toISOString() };
}

export interface MessageWindow {
  after?: number;
  before?: number;
  limit?: number;
}

/**
 * 房间的一段消息，按序号从旧到新：有 `after` 时取它之后最早的一批，
 * 否则取 `before` 之前（没有 `before` 时是全部）最新的一批。
 */
export async function listMessages(
  db: Database,
  roomId: RoomId,
  window: MessageWindow = {},
): Promise<Array<MessageView & { roomId: RoomId }>> {
  const [room] = await db.select({ id: rooms.id }).from(rooms).where(eq(rooms.id, roomId));
  if (!room) throw new RequestError(404, "房间不存在");

  const limit = window.limit ?? MESSAGE_PAGE_DEFAULT;
  const rows =
    window.after !== undefined
      ? await selectMessages(db)
          .where(and(eq(messages.roomId, roomId), gt(messages.seq, window.after)))
          .orderBy(asc(messages.seq))
          .limit(limit)
      : (
          await selectMessages(db)
            .where(
              and(
                eq(messages.roomId, roomId),
                window.before === undefined ? undefined : lt(messages.seq, window.before),
              ),
            )
            .orderBy(desc(messages.seq))
            .limit(limit)
        ).reverse();
  return rows.map((row) => ({ ...toView(row), roomId }));
}

/**
 * 取出 Agent 在各房间已读位置之后的全部消息，并把已投递位置推进到本次的最后一条。
 * 每个房间附上名字与成员。没有未读消息的房间不出现。
 */
export async function readInbox(db: Database, agentId: AgentId): Promise<InboxRoom[]> {
  return db.transaction(async (tx) => {
    const rows = await tx
      .select({ ...messageColumns, kind: rooms.kind, name: rooms.name, mentionsYou: messageMentions.agentId })
      .from(agentReadCursors)
      .innerJoin(rooms, eq(rooms.id, agentReadCursors.roomId))
      .innerJoin(
        messages,
        and(eq(messages.roomId, agentReadCursors.roomId), gt(messages.seq, agentReadCursors.lastReadSeq)),
      )
      .leftJoin(users, eq(users.id, messages.authorUserId))
      .leftJoin(agents, eq(agents.id, messages.authorAgentId))
      .leftJoin(messageMentions, and(eq(messageMentions.messageId, messages.id), eq(messageMentions.agentId, agentId)))
      .where(eq(agentReadCursors.agentId, agentId))
      .orderBy(asc(messages.roomId), asc(messages.seq));

    const byRoom = new Map<RoomId, { kind: RoomKind; name: string | null; messages: InboxRoom["messages"] }>();
    for (const row of rows) {
      const room = byRoom.get(row.roomId) ?? { kind: row.kind, name: row.name, messages: [] };
      room.messages.push({ ...toView(row), mentionsYou: row.mentionsYou !== null });
      byRoom.set(row.roomId, room);
    }
    const roomIds = [...byRoom.keys()];
    if (roomIds.length === 0) return [];

    for (const [roomId, room] of byRoom) {
      const last = room.messages.at(-1)?.seq ?? 0;
      await tx
        .update(agentReadCursors)
        .set({ deliveredSeq: sql`GREATEST(${agentReadCursors.deliveredSeq}, ${last})` })
        .where(and(eq(agentReadCursors.agentId, agentId), eq(agentReadCursors.roomId, roomId)));
    }

    const members = await roomMembers(tx, roomIds);
    return roomIds.map((roomId) => {
      const room = byRoom.get(roomId);
      if (!room) throw new Error("unreachable");
      return { roomId, kind: room.kind, name: room.name, members: members.get(roomId) ?? [], messages: room.messages };
    });
  });
}

/** 各房间的成员：本机用户在前，Agent 按名字排列。 */
async function roomMembers(tx: Transaction, roomIds: RoomId[]): Promise<Map<RoomId, Participant[]>> {
  const result = new Map<RoomId, Participant[]>();
  const add = (roomId: RoomId, member: Participant) => {
    result.set(roomId, [...(result.get(roomId) ?? []), member]);
  };
  const people = await tx
    .select({ roomId: roomUsers.roomId, id: users.id, displayName: users.displayName })
    .from(roomUsers)
    .innerJoin(users, eq(users.id, roomUsers.userId))
    .where(inArray(roomUsers.roomId, roomIds));
  for (const { roomId, ...person } of people) add(roomId, { kind: "user", ...person, handle: null });
  const bots = await tx
    .select({ roomId: roomAgents.roomId, id: agents.id, displayName: agents.displayName, handle: agents.handle })
    .from(roomAgents)
    .innerJoin(agents, eq(agents.id, roomAgents.agentId))
    .where(inArray(roomAgents.roomId, roomIds))
    .orderBy(asc(agents.displayName));
  for (const { roomId, ...bot } of bots) add(roomId, { kind: "agent", ...bot });
  return result;
}

/** Turn 成功后：Agent 每个房间的已读位置推进到已投递位置。 */
export async function acknowledge(db: Database, agentId: AgentId): Promise<void> {
  await db
    .update(agentReadCursors)
    .set({ lastReadSeq: agentReadCursors.deliveredSeq })
    .where(eq(agentReadCursors.agentId, agentId));
}
