import {
  type AgentId,
  isSendBack,
  type MessageId,
  type Notice,
  type RoomId,
  TASK_TITLE_MAX,
  TASK_TRANSITIONS,
  type TaskRefusal,
  type TaskStatus,
  type TaskView,
  taskRefusalText,
  taskStatusLabel,
} from "@crew/protocol";
import { and, asc, eq, isNotNull, isNull, or, sql } from "drizzle-orm";
import { alias } from "drizzle-orm/pg-core";
import type { Database } from "./db";
import { agents, messages, roomAgents, rooms, tasks } from "./db/schema";
import { RequestError } from "./errors";
import { type Author, assertMember, type PostResult, postMessageIn, type Transaction } from "./messages";

// 任务：房间里一条消息变成的待办。每次改动与它的通知写在同一个事务里：通知是任务的历史，
// 分配与退回的通知 @ 负责人、唤醒它。取舍见 Agent Note：任务（2026-10-05-tasks）。

type TaskRow = typeof tasks.$inferSelect;

/** 一次任务操作的结果：改动后的任务，以及要在提交后通知的消息（宿主消息与通知）。 */
export interface TaskChange {
  task: TaskView;
  posts: Array<Extract<PostResult, { kind: "posted" }>>;
}

/** 指定负责人：界面给 Agent ID，Agent 给 handle。 */
export type AgentRef = { id: AgentId } | { handle: string };

function refuse(refusal: TaskRefusal): never {
  const status = refusal.code === "not_found" || refusal.code === "message_not_found" ? 404 : 409;
  throw new RequestError(status, taskRefusalText(refusal), refusal);
}

/** 任务所在的房间。给的是讨论串时换成它所在的群聊：Agent 在讨论串里也能直接操作任务。 */
async function taskRoom(tx: Transaction, roomId: RoomId): Promise<{ id: RoomId; kind: "direct" | "group" }> {
  const [room] = await tx
    .select({ id: rooms.id, kind: rooms.kind, parentRoomId: rooms.parentRoomId })
    .from(rooms)
    .where(eq(rooms.id, roomId));
  if (!room) throw new RequestError(404, "房间不存在");
  if (room.kind === "thread") {
    if (!room.parentRoomId) throw new Error(`讨论串 ${roomId} 没有所在的群聊`);
    return taskRoom(tx, room.parentRoomId);
  }
  return { id: room.id, kind: room.kind };
}

/** 房间里的一个 Agent 成员。不是成员时拒绝。 */
async function memberAgent(
  tx: Transaction,
  roomId: RoomId,
  ref: AgentRef,
): Promise<{ id: AgentId; handle: string; displayName: string }> {
  const [agent] = await tx
    .select({ id: agents.id, handle: agents.handle, displayName: agents.displayName, member: roomAgents.agentId })
    .from(agents)
    .leftJoin(roomAgents, and(eq(roomAgents.agentId, agents.id), eq(roomAgents.roomId, roomId)))
    .where("id" in ref ? eq(agents.id, ref.id) : eq(agents.handle, ref.handle.toLowerCase()));
  const handle = agent?.handle ?? ("handle" in ref ? ref.handle : ref.id);
  if (!agent?.member) refuse({ code: "not_member", handle });
  return agent;
}

async function findTask(tx: Transaction, roomId: RoomId, number: number): Promise<TaskRow> {
  const [task] = await tx
    .select()
    .from(tasks)
    .where(and(eq(tasks.roomId, roomId), eq(tasks.number, number)));
  if (!task) refuse({ code: "not_found", number });
  return task;
}

/**
 * 写一条任务通知。群聊里发到宿主消息的讨论串（还没有时创建），私聊没有讨论串，发到时间线。
 * 正文里的 @ 照常记录：被 @ 到的负责人关注讨论串并被唤醒。
 */
async function notice(
  tx: Transaction,
  room: { id: RoomId; kind: "direct" | "group" },
  task: TaskRow,
  actor: Author,
  body: string,
  data: Notice,
): Promise<Extract<PostResult, { kind: "posted" }>> {
  const result = await postMessageIn(tx, room.id, actor, body, {
    kind: "system",
    notice: data,
    threadOf: room.kind === "group" ? task.messageId : undefined,
  });
  if (result.kind !== "posted") throw new Error("通知被拦下");
  return result;
}

/** 新任务的编号：房间里最大的编号加一。调用方已锁住房间行，同一房间的新建排队。 */
async function nextNumber(tx: Transaction, roomId: RoomId): Promise<number> {
  const [row] = await tx
    .select({ max: sql<number | null>`max(${tasks.number})` })
    .from(tasks)
    .where(eq(tasks.roomId, roomId));
  return (row?.max ?? 0) + 1;
}

const creator = (actor: Author) =>
  actor.kind === "user" ? { createdByUserId: actor.id } : { createdByAgentId: actor.id };

const mention = (agent: { handle: string } | undefined) => (agent ? `，分配给 @${agent.handle}` : "");

/**
 * 新建任务：以 `actor` 的身份在房间里发一条正文为标题的消息，再把它变成任务。
 * 分配了负责人时，标题消息不唤醒任何 Agent，由通知唤醒负责人；没有负责人时照普通消息的规则唤醒。
 */
export async function createTask(
  db: Database,
  actor: Author,
  roomId: RoomId,
  input: { title: string; assignee?: AgentRef },
): Promise<TaskChange> {
  return db.transaction(async (tx) => {
    const room = await taskRoom(tx, roomId);
    await assertMember(tx, room.id, actor);
    const assignee = input.assignee && (await memberAgent(tx, room.id, input.assignee));
    const host = await postMessageIn(tx, room.id, actor, input.title, { action: true, quiet: assignee !== undefined });
    if (host.kind !== "posted") throw new Error("任务的标题消息被拦下");
    const task = await insertTask(tx, room.id, actor, host.message.id, input.title, assignee?.id);
    const posted = await notice(tx, room, task, actor, `新建了任务 #${task.number}${mention(assignee)}`, {
      type: "task.created",
      number: task.number,
      assignee: assignee?.handle ?? null,
    });
    return { task: await viewOf(tx, task.id), posts: [host, posted] };
  });
}

/** 把房间里一条已有的消息转成任务。标题取正文的第一行（去掉空行），太长时截短。 */
export async function convertToTask(
  db: Database,
  actor: Author,
  roomId: RoomId,
  input: { messageId: MessageId; assignee?: AgentRef },
): Promise<TaskChange> {
  return db.transaction(async (tx) => {
    const [where] = await tx.select({ kind: rooms.kind }).from(rooms).where(eq(rooms.id, roomId));
    if (!where) throw new RequestError(404, "房间不存在");
    if (where.kind === "thread") refuse({ code: "in_thread" });
    const room = await taskRoom(tx, roomId);
    await assertMember(tx, room.id, actor);
    // 锁住房间行：与写消息、新建任务排队，编号不重复。
    await tx.select({ id: rooms.id }).from(rooms).where(eq(rooms.id, room.id)).for("update");
    const [message] = await tx
      .select({ kind: messages.kind, body: messages.body })
      .from(messages)
      .where(and(eq(messages.id, input.messageId), eq(messages.roomId, room.id)));
    if (!message) refuse({ code: "message_not_found" });
    if (message.kind === "system") refuse({ code: "system_message" });
    const [existing] = await tx
      .select({ number: tasks.number })
      .from(tasks)
      .where(eq(tasks.messageId, input.messageId));
    if (existing) refuse({ code: "already_task", number: existing.number });

    const assignee = input.assignee && (await memberAgent(tx, room.id, input.assignee));
    const task = await insertTask(tx, room.id, actor, input.messageId, titleOf(message.body), assignee?.id);
    const posted = await notice(tx, room, task, actor, `把这条消息转成任务 #${task.number}${mention(assignee)}`, {
      type: "task.converted",
      number: task.number,
      assignee: assignee?.handle ?? null,
    });
    return { task: await viewOf(tx, task.id), posts: [posted] };
  });
}

/** 消息正文的第一个非空行，截到标题的上限。 */
export function titleOf(body: string): string {
  const line = body.split("\n").find((candidate) => candidate.trim().length > 0) ?? body;
  return [...line.trim()].slice(0, TASK_TITLE_MAX).join("");
}

async function insertTask(
  tx: Transaction,
  roomId: RoomId,
  actor: Author,
  messageId: MessageId,
  title: string,
  assigneeId: AgentId | undefined,
): Promise<TaskRow> {
  const number = await nextNumber(tx, roomId);
  const [task] = await tx
    .insert(tasks)
    .values({ roomId, number, title, messageId, assigneeAgentId: assigneeId ?? null, ...creator(actor) })
    .returning();
  if (!task) throw new Error("写入任务失败");
  return task;
}

/**
 * 领取：负责人设成自己，待办推进到进行中。是一次带条件的更新：只有待办、并且没有负责人或负责人就是自己时成功，
 * 两个 Agent 同时领取时只有一个成功，另一个得到写明原因的拒绝。
 */
export async function claimTask(db: Database, agentId: AgentId, roomId: RoomId, number: number): Promise<TaskChange> {
  return db.transaction(async (tx) => {
    const room = await taskRoom(tx, roomId);
    const actor: Author = { kind: "agent", id: agentId };
    await assertMember(tx, room.id, actor);
    const current = await findTask(tx, room.id, number);
    if (current.status === "in_progress" && current.assigneeAgentId === agentId) {
      return { task: await viewOf(tx, current.id), posts: [] };
    }

    const [claimed] = await tx
      .update(tasks)
      .set({ assigneeAgentId: agentId, status: "in_progress", claimedAt: sql`now()`, updatedAt: sql`now()` })
      .where(
        and(
          eq(tasks.id, current.id),
          eq(tasks.status, "todo"),
          or(isNull(tasks.assigneeAgentId), eq(tasks.assigneeAgentId, agentId)),
        ),
      )
      .returning();
    if (!claimed) {
      // 没有更新到：读出现在的样子说明原因。并发时这里读到的是先领到的那一次提交后的结果。
      const now = await findTask(tx, room.id, number);
      const holder =
        now.assigneeAgentId && now.assigneeAgentId !== agentId ? await handleOf(tx, now.assigneeAgentId) : undefined;
      if (holder) refuse({ code: "claimed", number, by: holder });
      refuse({ code: "not_claimable", number, status: now.status });
    }
    const posted = await notice(tx, room, claimed, actor, `领取了 #${number}，待办 → 进行中`, {
      type: "task.claimed",
      number,
    });
    return { task: await viewOf(tx, claimed.id), posts: [posted] };
  });
}

/**
 * 改状态：按流转表检查，带条件地更新（状态仍是读到的那个）。进行中与待审要有负责人。
 * 别人把负责人的任务退回（待审或完成退回到进行中或待办）时，通知 @ 负责人。
 */
export async function setTaskStatus(
  db: Database,
  actor: Author,
  roomId: RoomId,
  number: number,
  status: TaskStatus,
): Promise<TaskChange> {
  return db.transaction(async (tx) => {
    const room = await taskRoom(tx, roomId);
    await assertMember(tx, room.id, actor);
    const current = await findTask(tx, room.id, number);
    if (current.status === status) return { task: await viewOf(tx, current.id), posts: [] };
    if (!TASK_TRANSITIONS[current.status].includes(status)) {
      refuse({ code: "transition", number, from: current.status, to: status });
    }
    if ((status === "in_progress" || status === "in_review") && !current.assigneeAgentId) {
      refuse({ code: "needs_assignee", number });
    }

    const [updated] = await tx
      .update(tasks)
      .set({ status, completedAt: status === "done" ? sql`now()` : null, updatedAt: sql`now()` })
      .where(
        and(
          eq(tasks.id, current.id),
          eq(tasks.status, current.status),
          // 进行中与待审要有负责人：同时有人取消了负责人时不更新，报“刚被改过”，而不是撞上数据库约束。
          status === "in_progress" || status === "in_review" ? isNotNull(tasks.assigneeAgentId) : undefined,
        ),
      )
      .returning();
    if (!updated) refuse({ code: "changed", number });

    const assignee = updated.assigneeAgentId;
    const byOther = !(actor.kind === "agent" && actor.id === assignee);
    const ping = assignee && byOther && isSendBack(current.status, status) ? `，@${await handleOf(tx, assignee)}` : "";
    const text = `把 #${number} 从${taskStatusLabel(current.status)}改成${taskStatusLabel(status)}${ping}`;
    const posted = await notice(tx, room, updated, actor, text, {
      type: "task.status",
      number,
      from: current.status,
      to: status,
      sentBack: ping !== "",
    });
    return { task: await viewOf(tx, updated.id), posts: [posted] };
  });
}

/**
 * 换负责人。只换人、不改状态；取消负责人时，进行中或待审的任务回到待办（它们必须有负责人）。
 * 完成或关闭的任务不再分配。通知 @ 新的负责人，唤醒它。
 */
export async function assignTask(
  db: Database,
  actor: Author,
  roomId: RoomId,
  number: number,
  target: AgentRef | null,
): Promise<TaskChange> {
  return db.transaction(async (tx) => {
    const room = await taskRoom(tx, roomId);
    await assertMember(tx, room.id, actor);
    const current = await findTask(tx, room.id, number);
    if (current.status === "done" || current.status === "closed") {
      refuse({ code: "finished", number, status: current.status });
    }
    const assignee = target ? await memberAgent(tx, room.id, target) : undefined;
    if ((assignee?.id ?? null) === current.assigneeAgentId) return { task: await viewOf(tx, current.id), posts: [] };

    const backToTodo = !assignee && (current.status === "in_progress" || current.status === "in_review");
    const [updated] = await tx
      .update(tasks)
      .set({
        assigneeAgentId: assignee?.id ?? null,
        ...(backToTodo ? { status: "todo" as const, claimedAt: null } : {}),
        updatedAt: sql`now()`,
      })
      .where(
        and(
          eq(tasks.id, current.id),
          eq(tasks.status, current.status),
          current.assigneeAgentId ? eq(tasks.assigneeAgentId, current.assigneeAgentId) : isNull(tasks.assigneeAgentId),
        ),
      )
      .returning();
    if (!updated) refuse({ code: "changed", number });

    const text = assignee
      ? `把 #${number} 分配给 @${assignee.handle}`
      : `取消了 #${number} 的负责人${backToTodo ? "，退回待办" : ""}`;
    const posted = await notice(tx, room, updated, actor, text, {
      type: "task.assigned",
      number,
      assignee: assignee?.handle ?? null,
    });
    return { task: await viewOf(tx, updated.id), posts: [posted] };
  });
}

async function handleOf(tx: Transaction, agentId: AgentId): Promise<string> {
  const [agent] = await tx.select({ handle: agents.handle }).from(agents).where(eq(agents.id, agentId));
  return agent?.handle ?? agentId;
}

const assignees = alias(agents, "assignees");
const threads = alias(rooms, "threads");

function selectTasks(db: Database | Transaction) {
  return db
    .select({
      id: tasks.id,
      roomId: tasks.roomId,
      number: tasks.number,
      title: tasks.title,
      status: tasks.status,
      assigneeId: assignees.id,
      assigneeName: assignees.displayName,
      assigneeHandle: assignees.handle,
      messageId: tasks.messageId,
      threadId: threads.id,
      createdAt: tasks.createdAt,
      updatedAt: tasks.updatedAt,
    })
    .from(tasks)
    .leftJoin(assignees, eq(assignees.id, tasks.assigneeAgentId))
    .leftJoin(threads, eq(threads.parentMessageId, tasks.messageId))
    .$dynamic();
}

type SelectedTask = Awaited<ReturnType<ReturnType<typeof selectTasks>["execute"]>>[number];

function toView(row: SelectedTask): TaskView {
  return {
    id: row.id,
    roomId: row.roomId,
    number: row.number,
    title: row.title,
    status: row.status,
    assignee:
      row.assigneeId && row.assigneeName && row.assigneeHandle
        ? { id: row.assigneeId, displayName: row.assigneeName, handle: row.assigneeHandle }
        : null,
    messageId: row.messageId,
    threadId: row.threadId,
    createdAt: row.createdAt.toISOString(),
    updatedAt: row.updatedAt.toISOString(),
  };
}

async function viewOf(tx: Transaction, taskId: string): Promise<TaskView> {
  const [row] = await selectTasks(tx).where(eq(tasks.id, taskId));
  if (!row) throw new Error(`任务 ${taskId} 不存在`);
  return toView(row);
}

/** 房间里的全部任务，按编号排列。给的是讨论串时列它所在群聊的任务。只有房间成员能看。 */
export async function listTasks(db: Database, actor: Author, roomId: RoomId): Promise<TaskView[]> {
  return db.transaction(async (tx) => {
    const room = await taskRoom(tx, roomId);
    await assertMember(tx, room.id, actor);
    return (await selectTasks(tx).where(eq(tasks.roomId, room.id)).orderBy(asc(tasks.number))).map(toView);
  });
}
