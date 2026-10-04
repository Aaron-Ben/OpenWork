import type { AgentId, RoomId, UserId } from "@crew/protocol";
import { asc, eq } from "drizzle-orm";
import type { Database } from "./db";
import { agentReadCursors, agents, roomAgents, rooms, roomUsers, userReadCursors } from "./db/schema";
import { RequestError } from "./errors";

/** 第 1 版只接 OpenCode。 */
export const DEFAULT_ENGINE_ID = "opencode";

export interface NewAgent {
  displayName: string;
  handle: string;
  persona: string;
  model: string;
}

export interface AgentSummary {
  id: AgentId;
  displayName: string;
  handle: string;
  persona: string;
  engineId: string;
  model: string;
  /** 本机用户与这个 Agent 的私聊房间。 */
  roomId: RoomId;
  createdAt: Date;
}

/**
 * 创建 Agent，并在同一个事务里建好它与本机用户的私聊房间、双方的成员关系与已读位置。
 *
 * @throws RequestError 409：handle 已被使用。
 */
export async function createAgent(db: Database, localUserId: UserId, input: NewAgent): Promise<AgentSummary> {
  return db.transaction(async (tx) => {
    // 本机只有一个用户，两个请求同时用同一个 handle 的竞争可以忽略：唯一约束仍会拒绝后到的那个。
    const [taken] = await tx.select({ id: agents.id }).from(agents).where(eq(agents.handle, input.handle));
    if (taken) throw new RequestError(409, `handle @${input.handle} 已被使用`);

    const [agent] = await tx
      .insert(agents)
      .values({ ...input, engineId: DEFAULT_ENGINE_ID })
      .returning();
    if (!agent) throw new Error("创建 Agent 失败");

    const [room] = await tx
      .insert(rooms)
      .values({ kind: "direct", directKey: `${localUserId}:${agent.id}` })
      .returning({ id: rooms.id });
    if (!room) throw new Error("创建私聊房间失败");

    await tx.insert(roomUsers).values({ roomId: room.id, userId: localUserId });
    await tx.insert(userReadCursors).values({ roomId: room.id, userId: localUserId });
    await tx.insert(roomAgents).values({ roomId: room.id, agentId: agent.id });
    await tx.insert(agentReadCursors).values({ agentId: agent.id, roomId: room.id });

    return { ...agent, roomId: room.id };
  });
}

/** @throws RequestError 404：Agent 不存在。 */
export async function assertAgentExists(db: Database, agentId: AgentId): Promise<void> {
  const [agent] = await db.select({ id: agents.id }).from(agents).where(eq(agents.id, agentId));
  if (!agent) throw new RequestError(404, "Agent 不存在");
}

/** 全部 Agent，按创建时间排列，附带各自的私聊房间。 */
export async function listAgents(db: Database): Promise<AgentSummary[]> {
  return db
    .select({
      id: agents.id,
      displayName: agents.displayName,
      handle: agents.handle,
      persona: agents.persona,
      engineId: agents.engineId,
      model: agents.model,
      roomId: roomAgents.roomId,
      createdAt: agents.createdAt,
    })
    .from(agents)
    .innerJoin(roomAgents, eq(roomAgents.agentId, agents.id))
    .innerJoin(rooms, eq(rooms.id, roomAgents.roomId))
    .where(eq(rooms.kind, "direct"))
    .orderBy(asc(agents.createdAt));
}
