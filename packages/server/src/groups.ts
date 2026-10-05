import type { AgentId, RoomId, UserId } from "@crew/protocol";
import { and, asc, eq, inArray, sql } from "drizzle-orm";
import type { Database } from "./db";
import { agentReadCursors, agents, roomAgents, rooms, roomUsers, userReadCursors } from "./db/schema";
import { RequestError } from "./errors";
import { mutedNow } from "./messages";

export interface GroupSummary {
  id: RoomId;
  name: string;
  agentIds: AgentId[];
  /** 现在静音着这个群的 Agent；时间是 PostgreSQL 写出的字符串。 */
  mutes: Array<{ agentId: AgentId; since: string; until: string | null }>;
  createdAt: Date;
}

type Transaction = Parameters<Parameters<Database["transaction"]>[0]>[0];

/** @throws RequestError 404：其中有不存在的 Agent。 */
async function assertAgentsExist(tx: Transaction, agentIds: AgentId[]): Promise<void> {
  const found = await tx.select({ id: agents.id }).from(agents).where(inArray(agents.id, agentIds));
  if (found.length !== new Set(agentIds).size) throw new RequestError(404, "Agent 不存在");
}

/**
 * 把 Agent 加进房间：成员关系与已读位置。已读与已投递位置都从房间当前的最新序号开始，
 * 新成员看不到加入前的消息。已经在房间里的 Agent 不变。调用方已锁住房间行。
 */
async function addAgents(tx: Transaction, roomId: RoomId, latestSeq: number, agentIds: AgentId[]): Promise<void> {
  await tx
    .insert(roomAgents)
    .values(agentIds.map((agentId) => ({ roomId, agentId })))
    .onConflictDoNothing();
  await tx
    .insert(agentReadCursors)
    .values(agentIds.map((agentId) => ({ agentId, roomId, lastReadSeq: latestSeq, deliveredSeq: latestSeq })))
    .onConflictDoNothing();
}

/** 新建群聊：本机用户与选中的 Agent 都是成员。 */
export async function createGroup(
  db: Database,
  localUserId: UserId,
  input: { name: string; agentIds: AgentId[] },
): Promise<GroupSummary> {
  return db.transaction(async (tx) => {
    await assertAgentsExist(tx, input.agentIds);
    const [room] = await tx.insert(rooms).values({ kind: "group", name: input.name }).returning();
    if (!room) throw new Error("创建群聊失败");
    await tx.insert(roomUsers).values({ roomId: room.id, userId: localUserId });
    await tx.insert(userReadCursors).values({ roomId: room.id, userId: localUserId });
    await addAgents(tx, room.id, room.nextSeq, input.agentIds);
    return groupOf(tx, room.id);
  });
}

/** @throws RequestError 404：群聊或其中某个 Agent 不存在。 */
export async function addGroupMembers(db: Database, roomId: RoomId, agentIds: AgentId[]): Promise<GroupSummary> {
  return db.transaction(async (tx) => {
    // 锁住房间行：与写消息互斥，新成员的起点正好是加入时的最新序号。
    const [room] = await tx
      .select({ nextSeq: rooms.nextSeq })
      .from(rooms)
      .where(and(eq(rooms.id, roomId), eq(rooms.kind, "group")))
      .for("update");
    if (!room) throw new RequestError(404, "群聊不存在");
    await assertAgentsExist(tx, agentIds);
    await addAgents(tx, roomId, room.nextSeq, agentIds);
    return groupOf(tx, roomId);
  });
}

const groupColumns = {
  id: rooms.id,
  name: sql<string>`${rooms.name}`,
  agentIds: sql<
    AgentId[]
  >`coalesce(array_agg(${roomAgents.agentId} ORDER BY ${roomAgents.agentId}) FILTER (WHERE ${roomAgents.agentId} IS NOT NULL), '{}')`,
  mutes: sql<
    GroupSummary["mutes"]
  >`coalesce(json_agg(json_build_object('agentId', ${roomAgents.agentId}, 'since', ${roomAgents.mutedAt}, 'until', ${roomAgents.mutedUntil}) ORDER BY ${roomAgents.agentId}) FILTER (WHERE ${mutedNow}), '[]')`,
  createdAt: rooms.createdAt,
};

export async function groupOf(tx: Transaction, roomId: RoomId): Promise<GroupSummary> {
  const [group] = await tx
    .select(groupColumns)
    .from(rooms)
    .leftJoin(roomAgents, eq(roomAgents.roomId, rooms.id))
    .where(eq(rooms.id, roomId))
    .groupBy(rooms.id);
  if (!group) throw new Error(`群聊 ${roomId} 不存在`);
  return group;
}

/** 全部群聊，按创建时间排列。 */
export async function listGroups(db: Database): Promise<GroupSummary[]> {
  return db
    .select(groupColumns)
    .from(rooms)
    .leftJoin(roomAgents, eq(roomAgents.roomId, rooms.id))
    .where(eq(rooms.kind, "group"))
    .groupBy(rooms.id)
    .orderBy(asc(rooms.createdAt));
}
