import { type AgentId, type MuteRefusal, type MuteState, muteRefusalText, type RoomId } from "@crew/protocol";
import { and, eq, sql } from "drizzle-orm";
import type { Database } from "./db";
import { agents, roomAgents, rooms } from "./db/schema";
import { RequestError } from "./errors";
import { type Author, assertMember, mutedNow, type PostResult, postMessageIn, type Transaction } from "./messages";
import { clockText } from "./reminders";

// 静音：Agent 让一个群的消息不再唤醒它。只有 Agent 自己能静音，用户只能解除。静音与解除都在群里写一行通知，
// 到期不写。时间都用数据库的 now()：唤醒与收件箱按它判断是否到期。
// 取舍见 Agent Note：提醒、记忆与静音（2026-10-05-reminders-memory-mute）决策 7、9、10。

type Posted = Extract<PostResult, { kind: "posted" }>;

function refuse(refusal: MuteRefusal): never {
  throw new RequestError(409, muteRefusalText(refusal), refusal);
}

/** 只有群聊能静音：私聊不能，讨论串跟着它所在的群聊。 */
async function lockGroup(tx: Transaction, roomId: RoomId): Promise<void> {
  const [room] = await tx.select({ kind: rooms.kind }).from(rooms).where(eq(rooms.id, roomId)).for("update");
  if (!room) throw new RequestError(404, "房间不存在");
  if (room.kind === "direct") refuse({ code: "mute_direct" });
  if (room.kind === "thread") refuse({ code: "mute_thread" });
}

/** Agent 静音一个群，`minutes` 为空时一直静音。已经静音着时换成新的时长，也写一行通知。 */
export async function muteRoom(
  db: Database,
  agentId: AgentId,
  roomId: RoomId,
  minutes: number | undefined,
): Promise<{ state: MuteState; posts: Posted[] }> {
  return db.transaction(async (tx) => {
    await lockGroup(tx, roomId);
    await assertMember(tx, roomId, { kind: "agent", id: agentId });
    const [row] = await tx
      .update(roomAgents)
      .set({
        mutedAt: sql`now()`,
        mutedUntil: minutes === undefined ? null : sql`now() + make_interval(mins => ${minutes})`,
      })
      .where(and(eq(roomAgents.roomId, roomId), eq(roomAgents.agentId, agentId)))
      .returning({ mutedAt: roomAgents.mutedAt, mutedUntil: roomAgents.mutedUntil });
    if (!row?.mutedAt) throw new Error("静音没有写进去");
    const until = row.mutedUntil;
    const posted = await postMessageIn(
      tx,
      roomId,
      { kind: "agent", id: agentId },
      until ? `静音了这个群，到 ${clockText(until, row.mutedAt)}` : "静音了这个群",
      { kind: "system", notice: { type: "mute", until: until?.toISOString() ?? null } },
    );
    if (posted.kind !== "posted") throw new Error("静音的通知被拦下");
    return { state: { roomId, muted: true, until: until?.toISOString() ?? null }, posts: [posted] };
  });
}

/**
 * 解除 Agent 在群里的静音：Agent 自己解除，或用户替它解除。正静音着时写一行通知；没有静音或已经到期时什么也不写。
 */
export async function unmuteRoom(
  db: Database,
  actor: Author,
  roomId: RoomId,
  agentId: AgentId,
): Promise<{ state: MuteState; posts: Posted[] }> {
  return db.transaction(async (tx) => {
    await lockGroup(tx, roomId);
    await assertMember(tx, roomId, actor);
    const [current] = await tx
      .select({ muted: mutedNow, handle: agents.handle, name: agents.displayName })
      .from(roomAgents)
      .innerJoin(agents, eq(agents.id, roomAgents.agentId))
      .where(and(eq(roomAgents.roomId, roomId), eq(roomAgents.agentId, agentId)));
    if (!current) throw new RequestError(404, "这个 Agent 不在群里");
    await tx
      .update(roomAgents)
      .set({ mutedAt: null, mutedUntil: null })
      .where(and(eq(roomAgents.roomId, roomId), eq(roomAgents.agentId, agentId)));
    const state: MuteState = { roomId, muted: false, until: null };
    if (!current.muted) return { state, posts: [] };

    const self = actor.kind === "agent" && actor.id === agentId;
    const posted = await postMessageIn(
      tx,
      roomId,
      actor,
      // 正文写名字、不写 @：@ 会记成点到它，它以后再静音时，这条旧通知会被当成 @ 它而穿透静音。
      // 只是告知，不唤醒它，它下次被唤醒时会读到。
      self ? "解除了这个群的静音" : `解除了 ${current.name} 在这个群的静音`,
      { kind: "system", notice: { type: "unmute", handle: self ? null : current.handle }, quiet: true },
    );
    if (posted.kind !== "posted") throw new Error("解除静音的通知被拦下");
    return { state, posts: [posted] };
  });
}
