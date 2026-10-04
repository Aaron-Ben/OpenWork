import type { AgentId, Participant, RoomId, RoomKind, UserId } from "@crew/protocol";
import { and, desc, eq, sql } from "drizzle-orm";
import type { Database } from "./db";
import { agents, messages, roomAgents, rooms, roomUsers, userReadCursors, users } from "./db/schema";
import { RequestError } from "./errors";

/** 会话列表里最后一条消息的预览最多这么多字符。 */
export const PREVIEW_MAX = 200;

export interface Conversation {
  roomId: RoomId;
  kind: RoomKind;
  /** 群聊的名字；私聊是 Agent 的名字。 */
  name: string;
  agentIds: AgentId[];
  lastMessage: { author: Participant; body: string; createdAt: Date } | null;
  /** 别人发的、用户还没读的消息数。 */
  unread: number;
  /** 最后一条消息的时间；没有消息时是房间的创建时间。列表按它从新到旧排列。 */
  activeAt: Date;
}

/** 用户所在的全部房间，按最后活动从新到旧排列。 */
export async function listConversations(db: Database, userId: UserId): Promise<Conversation[]> {
  const unread = sql<number>`(
    SELECT count(*)::int FROM ${messages}
    WHERE ${messages.roomId} = ${rooms.id}
      AND ${messages.seq} > ${userReadCursors.lastReadSeq}
      AND ${messages.authorUserId} IS DISTINCT FROM ${userId}
  )`;
  const activeAt = sql<Date>`coalesce(${messages.createdAt}, ${rooms.createdAt})`;
  const rows = await db
    .select({
      roomId: rooms.id,
      kind: rooms.kind,
      groupName: rooms.name,
      body: messages.body,
      createdAt: messages.createdAt,
      authorUserId: messages.authorUserId,
      authorAgentId: messages.authorAgentId,
      userName: users.displayName,
      agentName: agents.displayName,
      agentHandle: agents.handle,
      unread,
      activeAt,
    })
    .from(roomUsers)
    .innerJoin(rooms, eq(rooms.id, roomUsers.roomId))
    .innerJoin(userReadCursors, and(eq(userReadCursors.roomId, rooms.id), eq(userReadCursors.userId, userId)))
    .leftJoin(messages, and(eq(messages.roomId, rooms.id), eq(messages.seq, rooms.nextSeq)))
    .leftJoin(users, eq(users.id, messages.authorUserId))
    .leftJoin(agents, eq(agents.id, messages.authorAgentId))
    .where(eq(roomUsers.userId, userId))
    .orderBy(desc(activeAt), desc(rooms.id));

  const members = await db
    .select({ roomId: roomAgents.roomId, agentId: agents.id, name: agents.displayName })
    .from(roomAgents)
    .innerJoin(agents, eq(agents.id, roomAgents.agentId))
    .orderBy(agents.createdAt);
  const membersOf = new Map<RoomId, Array<{ agentId: AgentId; name: string }>>();
  for (const { roomId, ...member } of members) membersOf.set(roomId, [...(membersOf.get(roomId) ?? []), member]);

  return rows.map((row) => {
    const roomMembers = membersOf.get(row.roomId) ?? [];
    const author: Participant | undefined = row.authorUserId
      ? { kind: "user", id: row.authorUserId, displayName: row.userName ?? "", handle: null }
      : row.authorAgentId
        ? { kind: "agent", id: row.authorAgentId, displayName: row.agentName ?? "", handle: row.agentHandle }
        : undefined;
    return {
      roomId: row.roomId,
      kind: row.kind,
      name: row.kind === "group" ? (row.groupName ?? "") : (roomMembers[0]?.name ?? ""),
      agentIds: roomMembers.map((member) => member.agentId),
      lastMessage:
        author && row.body !== null && row.createdAt
          ? { author, body: row.body.slice(0, PREVIEW_MAX), createdAt: row.createdAt }
          : null,
      unread: row.unread,
      activeAt: new Date(row.activeAt),
    };
  });
}

/**
 * 把用户在一个房间的已读位置推进到 `seq`。只前进；超过房间已有的序号时推进到最新一条。
 *
 * @throws RequestError 404：用户不在这个房间。
 */
export async function markRead(db: Database, userId: UserId, roomId: RoomId, seq: number): Promise<void> {
  const [row] = await db
    .update(userReadCursors)
    .set({
      lastReadSeq: sql`GREATEST(${userReadCursors.lastReadSeq}, LEAST(${seq}, (SELECT ${rooms.nextSeq} FROM ${rooms} WHERE ${rooms.id} = ${roomId})))`,
    })
    .where(and(eq(userReadCursors.userId, userId), eq(userReadCursors.roomId, roomId)))
    .returning({ roomId: userReadCursors.roomId });
  if (!row) throw new RequestError(404, "房间不存在");
}
