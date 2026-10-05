import {
  type AgentId,
  type InboxRoom,
  type MessageId,
  type MessageView,
  type Notice,
  type Participant,
  type RoomId,
  type TaskTag,
  THREAD_REFUSALS,
  type UserId,
} from "@crew/protocol";
import { and, asc, desc, eq, gt, inArray, isNull, lt, ne, or, sql } from "drizzle-orm";
import { alias } from "drizzle-orm/pg-core";
import type { Database } from "./db";
import {
  agentReadCursors,
  agents,
  messageMentions,
  messages,
  roomAgents,
  rooms,
  roomUsers,
  tasks,
  userReadCursors,
  users,
} from "./db/schema";
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
      /** 消息在讨论串里时，讨论串所在的群聊：它的讨论串摘要变了。 */
      parentRoomId: RoomId | null;
      /** 需要唤醒的 Agent，规则见 `wakeTargets` 与 `threadWakeTargets`。 */
      wakeAgentIds: AgentId[];
    }
  | {
      /** Agent 的回复被拦下：房间里有它还没看到的、别人发的消息。消息没有写入。 */
      kind: "held";
      roomId: RoomId;
      newMessages: MessageView[];
      omitted: number;
    };

export type Transaction = Parameters<Parameters<Database["transaction"]>[0]>[0];

/**
 * 写入一条消息。用户与 Agent 都经过这里。带 `threadOf` 时写到房间里这条消息的讨论串，讨论串还没有时创建。
 *
 * 在一个事务里：锁住房间行，确认作者是房间成员（讨论串看它所在群聊的成员）；作者是 Agent 时先做 HELD 检查；
 * 然后把房间的 `next_seq` 加一取得序号，写入消息与它 @ 到的 Agent。
 * 锁住房间行使同一房间的写入排队，序号连续且与提交顺序一致，HELD 检查也不会与新消息交错。
 * 调用方在事务提交后再发通知。
 */
export async function postMessage(
  db: Database,
  roomId: RoomId,
  author: Author,
  body: string,
  threadOf?: MessageId,
): Promise<PostResult> {
  return db.transaction((tx) => postMessageIn(tx, roomId, author, body, { threadOf }));
}

export interface PostOptions {
  /** 发到房间里这条消息的讨论串，讨论串还没有时创建。 */
  threadOf?: MessageId;
  /**
   * `system` 是通知：不做 HELD 检查（它记的是刚做完的操作，不是回复），不推进作者的已投递位置，
   * 只唤醒它 @ 到的 Agent。
   */
  kind?: "text" | "system";
  /** 通知的类型与数据（只给 `system` 用）。 */
  notice?: Notice;
  /**
   * 这条消息是一次操作的一部分，不是回复：新建任务时以创建者的身份发出的标题。不做 HELD 检查，
   * 也不推进作者的已投递位置。
   */
  action?: boolean;
  /** 不唤醒任何 Agent：新建任务时已经分配了负责人，由随后的通知唤醒它，不再唤醒群里的其他 Agent。 */
  quiet?: boolean;
  /**
   * 只唤醒这些 Agent，代替通常的规则：提醒到点时只唤醒它的主人，即使通知是主人自己写的。
   * 消息在讨论串里时，它们也关注这个讨论串，否则读不到这条消息。
   */
  wake?: AgentId[];
}

/** 在调用方的事务里写一条消息，规则同 `postMessage`。任务操作用它把通知与任务改动写在同一个事务里。 */
export async function postMessageIn(
  tx: Transaction,
  roomId: RoomId,
  author: Author,
  body: string,
  options: PostOptions = {},
): Promise<PostResult> {
  const kind = options.kind ?? "text";
  // 回复要经过 HELD 检查，并在发出后推进作者的已投递位置；通知与操作里发出的消息都不算回复。
  const isReply = kind === "text" && !options.action;
  {
    if (options.threadOf) roomId = await openThread(tx, roomId, options.threadOf, author);
    const [room] = await tx
      .select({ nextSeq: rooms.nextSeq, kind: rooms.kind, parentRoomId: rooms.parentRoomId })
      .from(rooms)
      .where(eq(rooms.id, roomId))
      .for("update");
    if (!room) throw new RequestError(404, "房间不存在");
    const membershipRoomId = room.parentRoomId ?? roomId;
    await assertMember(tx, membershipRoomId, author);

    if (author.kind === "agent" && isReply) {
      // 在讨论串里发言就关注它。新关注者的已读位置从 0 开始，HELD 会先给它看讨论串里已有的消息。
      if (room.kind === "thread") await follow(tx, roomId, [author.id]);
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
        kind,
        notice: options.notice ?? null,
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
      .where(eq(roomAgents.roomId, membershipRoomId));
    const memberIds = members.map((member) => member.id);
    const handles = new Set(mentionedHandles(body));
    const mentioned = members.filter((member) => handles.has(member.handle)).map((member) => member.id);
    if (mentioned.length > 0) {
      await tx.insert(messageMentions).values(mentioned.map((agentId) => ({ messageId: message.id, agentId })));
      if (room.kind === "thread") await follow(tx, roomId, mentioned);
    }

    if (options.wake && room.kind === "thread") await follow(tx, roomId, options.wake);
    if (author.kind === "agent" && isReply) {
      // 通过了 HELD 检查，这之前的消息不是看过的就是它自己发的。
      await tx
        .update(agentReadCursors)
        .set({ deliveredSeq: seq })
        .where(and(eq(agentReadCursors.agentId, author.id), eq(agentReadCursors.roomId, roomId)));
    }

    return {
      kind: "posted",
      message,
      parentRoomId: room.parentRoomId,
      wakeAgentIds: options.wake
        ? options.wake
        : options.quiet
          ? []
          : kind === "system"
            ? mentioned.filter((id) => author.kind !== "agent" || id !== author.id)
            : room.kind === "thread"
              ? await threadWakeTargets(tx, roomId, author, memberIds, mentioned)
              : wakeTargets(author, memberIds, mentioned),
    };
  }
}

/**
 * 找到或创建房间里一条消息下的讨论串，返回讨论串的房间 ID。只有群聊能开讨论串，讨论串里不能再开。
 * 创建时：群聊里的用户从 0 开始读；消息的作者是 Agent 时，它关注讨论串。
 */
async function openThread(tx: Transaction, roomId: RoomId, messageId: MessageId, author: Author): Promise<RoomId> {
  const [parent] = await tx.select({ kind: rooms.kind }).from(rooms).where(eq(rooms.id, roomId));
  if (!parent) throw new RequestError(404, "房间不存在");
  if (parent.kind === "direct") throw new RequestError(400, THREAD_REFUSALS.direct);
  if (parent.kind === "thread") throw new RequestError(400, THREAD_REFUSALS.nested);
  await assertMember(tx, roomId, author);
  const [message] = await tx
    .select({ authorAgentId: messages.authorAgentId })
    .from(messages)
    .where(and(eq(messages.id, messageId), eq(messages.roomId, roomId)));
  if (!message) throw new RequestError(404, THREAD_REFUSALS.noMessage);

  // 两个请求同时创建时，唯一约束让后到的等前一个提交，然后什么也不插入，下面读到前一个创建的。
  const [created] = await tx
    .insert(rooms)
    .values({ kind: "thread", parentRoomId: roomId, parentMessageId: messageId })
    .onConflictDoNothing({ target: rooms.parentMessageId })
    .returning({ id: rooms.id });
  if (!created) {
    const [existing] = await tx.select({ id: rooms.id }).from(rooms).where(eq(rooms.parentMessageId, messageId));
    if (!existing) throw new Error(`消息 ${messageId} 的讨论串不存在`);
    return existing.id;
  }

  const people = await tx.select({ userId: roomUsers.userId }).from(roomUsers).where(eq(roomUsers.roomId, roomId));
  if (people.length > 0) {
    await tx.insert(userReadCursors).values(people.map(({ userId }) => ({ userId, roomId: created.id })));
  }
  if (message.authorAgentId) await follow(tx, created.id, [message.authorAgentId]);
  return created.id;
}

/**
 * Agent 关注讨论串：在讨论串里有已读位置。已读位置从 0 开始，它下次被唤醒时能读到讨论串里已有的消息。
 * 已经关注的不变。
 */
async function follow(tx: Transaction, threadId: RoomId, agentIds: AgentId[]): Promise<void> {
  if (agentIds.length === 0) return;
  await tx
    .insert(agentReadCursors)
    .values(agentIds.map((agentId) => ({ agentId, roomId: threadId })))
    .onConflictDoNothing();
}

/**
 * 讨论串里的消息唤醒哪些 Agent：Agent 的消息与群聊一样，只唤醒它 @ 到的其他成员；
 * 用户的消息唤醒全部关注者，不唤醒群里的其他 Agent。还没有 Agent 关注时，群里的 Agent 全部关注并被唤醒，
 * 免得用户的问题没人回答。
 */
async function threadWakeTargets(
  tx: Transaction,
  threadId: RoomId,
  author: Author,
  memberIds: AgentId[],
  mentionedIds: AgentId[],
): Promise<AgentId[]> {
  if (author.kind === "agent") return wakeTargets(author, memberIds, mentionedIds);
  const followers = await tx
    .select({ agentId: agentReadCursors.agentId })
    .from(agentReadCursors)
    .where(eq(agentReadCursors.roomId, threadId));
  if (followers.length > 0) return followers.map((row) => row.agentId);
  await follow(tx, threadId, memberIds);
  return memberIds;
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
 * HELD 检查：房间里有已投递位置之后、别人发的消息（或自己的提醒）时，从最早的开始返回至多 `HELD_SHOWN_MAX` 条，
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

  // 自己写的消息不拦，只有自己的提醒例外：它以自己的名义写下，却是要唤醒自己去看的。一轮进行中提醒到点、
  // 这一轮又在同一房间回复时，不拦就会把已投递位置推过它，这一轮结束后它被当成已读，提醒唤醒的下一轮读不到。
  const unseen = and(
    eq(messages.roomId, roomId),
    gt(messages.seq, cursor.deliveredSeq),
    or(
      isNull(messages.authorAgentId),
      ne(messages.authorAgentId, agentId),
      sql`${messages.notice}->>'type' = 'reminder'`,
    ),
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
  return { kind: "held", roomId, newMessages: rows.map(toView), omitted };
}

export async function assertMember(tx: Transaction, roomId: RoomId, author: Author): Promise<void> {
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
  kind: messages.kind,
  notice: messages.notice,
  body: messages.body,
  createdAt: messages.createdAt,
  authorUserId: messages.authorUserId,
  authorAgentId: messages.authorAgentId,
  userName: users.displayName,
  agentName: agents.displayName,
  agentHandle: agents.handle,
  runId: messages.runId,
  heldBefore: messages.heldBefore,
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
  kind: "text" | "system";
  notice: Notice | null;
  body: string;
  createdAt: Date;
  authorUserId: UserId | null;
  authorAgentId: AgentId | null;
  userName: string | null;
  agentName: string | null;
  agentHandle: string | null;
  runId: string | null;
  heldBefore: number;
};

function toView(row: MessageRow): MessageView {
  const author: Participant = row.authorUserId
    ? { kind: "user", id: row.authorUserId, displayName: row.userName ?? "", handle: null }
    : { kind: "agent", id: row.authorAgentId ?? "", displayName: row.agentName ?? "", handle: row.agentHandle };
  return {
    id: row.id,
    seq: row.seq,
    kind: row.kind,
    notice: row.notice,
    author,
    body: row.body,
    createdAt: row.createdAt.toISOString(),
  };
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
): Promise<Array<MessageView & { roomId: RoomId; runId: string | null; heldBefore: number }>> {
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
  return rows.map((row) => ({ ...toView(row), roomId, runId: row.runId, heldBefore: row.heldBefore }));
}

const parentRooms = alias(rooms, "parent_rooms");
const assignees = alias(agents, "assignees");

/** 宿主消息上的任务：编号、状态与负责人的 handle。不是任务的消息这几列为空。 */
const taskTagColumns = {
  taskNumber: tasks.number,
  taskStatus: tasks.status,
  taskAssignee: assignees.handle,
};

function taskTagOf(row: {
  taskNumber: number | null;
  taskStatus: TaskTag["status"] | null;
  taskAssignee: string | null;
}): TaskTag | null {
  return row.taskNumber === null || row.taskStatus === null
    ? null
    : { number: row.taskNumber, status: row.taskStatus, assignee: row.taskAssignee };
}

/**
 * 取出 Agent 在各房间已读位置之后的全部消息，并把已投递位置推进到本次的最后一条。
 * 每个房间附上名字与成员；讨论串附上它所在群聊的名字与成员，以及挂在下面的那条消息。没有未读消息的房间不出现。
 */
export async function readInbox(db: Database, agentId: AgentId): Promise<InboxRoom[]> {
  return db.transaction(async (tx) => {
    const rows = await tx
      .select({
        ...messageColumns,
        roomKind: rooms.kind,
        name: sql<string | null>`coalesce(${parentRooms.name}, ${rooms.name})`,
        ...taskTagColumns,
        parentRoomId: rooms.parentRoomId,
        parentMessageId: rooms.parentMessageId,
        mentionsYou: messageMentions.agentId,
      })
      .from(agentReadCursors)
      .innerJoin(rooms, eq(rooms.id, agentReadCursors.roomId))
      .leftJoin(parentRooms, eq(parentRooms.id, rooms.parentRoomId))
      .innerJoin(
        messages,
        and(eq(messages.roomId, agentReadCursors.roomId), gt(messages.seq, agentReadCursors.lastReadSeq)),
      )
      .leftJoin(users, eq(users.id, messages.authorUserId))
      .leftJoin(agents, eq(agents.id, messages.authorAgentId))
      .leftJoin(messageMentions, and(eq(messageMentions.messageId, messages.id), eq(messageMentions.agentId, agentId)))
      .leftJoin(tasks, eq(tasks.messageId, messages.id))
      .leftJoin(assignees, eq(assignees.id, tasks.assigneeAgentId))
      .where(eq(agentReadCursors.agentId, agentId))
      .orderBy(asc(messages.roomId), asc(messages.seq));

    type Group = Omit<InboxRoom, "roomId" | "members" | "parent"> & {
      parentRoomId: RoomId | null;
      parentMessageId: MessageId | null;
    };
    const byRoom = new Map<RoomId, Group>();
    for (const row of rows) {
      const room = byRoom.get(row.roomId) ?? {
        kind: row.roomKind,
        name: row.name,
        parentRoomId: row.parentRoomId,
        parentMessageId: row.parentMessageId,
        messages: [],
      };
      room.messages.push({ ...toView(row), mentionsYou: row.mentionsYou !== null, task: taskTagOf(row) });
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

    const groups = [...byRoom.values()];
    const membershipOf = (roomId: RoomId) => byRoom.get(roomId)?.parentRoomId ?? roomId;
    const members = await roomMembers(tx, [...new Set(roomIds.map(membershipOf))]);
    const parentIds = groups.flatMap((room) => (room.parentMessageId ? [room.parentMessageId] : []));
    const parents =
      parentIds.length === 0
        ? new Map<MessageId, MessageView & { task: TaskTag | null }>()
        : new Map(
            (
              await tx
                .select({ ...messageColumns, ...taskTagColumns })
                .from(messages)
                .leftJoin(users, eq(users.id, messages.authorUserId))
                .leftJoin(agents, eq(agents.id, messages.authorAgentId))
                .leftJoin(tasks, eq(tasks.messageId, messages.id))
                .leftJoin(assignees, eq(assignees.id, tasks.assigneeAgentId))
                .where(inArray(messages.id, parentIds))
            ).map((row) => [row.id, { ...toView(row), task: taskTagOf(row) }]),
          );

    return roomIds.map((roomId): InboxRoom => {
      const room = byRoom.get(roomId);
      if (!room) throw new Error("unreachable");
      const { parentRoomId, parentMessageId, ...rest } = room;
      const parentMessage = parentMessageId ? parents.get(parentMessageId) : undefined;
      return {
        roomId,
        ...rest,
        members: members.get(membershipOf(roomId)) ?? [],
        parent: parentRoomId && parentMessage ? { roomId: parentRoomId, message: parentMessage } : null,
      };
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

export interface ThreadSummaryRow {
  id: RoomId;
  parent: MessageView;
  replies: number;
  lastReplyAt: Date | null;
  participants: Participant[];
  unread: number;
}

/**
 * 群聊里的全部讨论串，按创建先后排列：回复数、最后回复时间、发过言的人（按第一次发言排列，至多 `participantsMax` 个）
 * 与用户还没读的、别人发的回复数。
 *
 * @throws RequestError 404：房间不存在。
 */
export async function listThreads(
  db: Database,
  roomId: RoomId,
  userId: UserId,
  participantsMax: number,
): Promise<ThreadSummaryRow[]> {
  const [room] = await db.select({ id: rooms.id }).from(rooms).where(eq(rooms.id, roomId));
  if (!room) throw new RequestError(404, "房间不存在");

  const threads = await db
    .select({
      id: rooms.id,
      parentMessageId: sql<MessageId>`${rooms.parentMessageId}`,
      replies: rooms.nextSeq,
      lastReplyAt: sql<Date | null>`(SELECT max(${messages.createdAt}) FROM ${messages} WHERE ${messages.roomId} = ${rooms.id})`,
      unread: sql<number>`(
        SELECT count(*)::int FROM ${messages}
        WHERE ${messages.roomId} = ${rooms.id}
          AND ${messages.seq} > ${userReadCursors.lastReadSeq}
          AND ${messages.authorUserId} IS DISTINCT FROM ${userId}
      )`,
    })
    .from(rooms)
    .innerJoin(userReadCursors, and(eq(userReadCursors.roomId, rooms.id), eq(userReadCursors.userId, userId)))
    .where(eq(rooms.parentRoomId, roomId))
    .orderBy(asc(rooms.createdAt));

  const firstSeq = sql<number>`min(${messages.seq})`;
  const authors = await db
    .select({
      roomId: messages.roomId,
      userId: messages.authorUserId,
      agentId: messages.authorAgentId,
      userName: users.displayName,
      agentName: agents.displayName,
      handle: agents.handle,
    })
    .from(messages)
    .innerJoin(rooms, eq(rooms.id, messages.roomId))
    .leftJoin(users, eq(users.id, messages.authorUserId))
    .leftJoin(agents, eq(agents.id, messages.authorAgentId))
    .where(eq(rooms.parentRoomId, roomId))
    .groupBy(
      messages.roomId,
      messages.authorUserId,
      messages.authorAgentId,
      users.displayName,
      agents.displayName,
      agents.handle,
    )
    .orderBy(firstSeq);
  const participants = new Map<RoomId, Participant[]>();
  for (const row of authors) {
    const list = participants.get(row.roomId) ?? [];
    if (list.length >= participantsMax) continue;
    list.push(
      row.userId
        ? { kind: "user", id: row.userId, displayName: row.userName ?? "", handle: null }
        : { kind: "agent", id: row.agentId ?? "", displayName: row.agentName ?? "", handle: row.handle },
    );
    participants.set(row.roomId, list);
  }

  const parentIds = threads.map((thread) => thread.parentMessageId);
  const parents = new Map(
    parentIds.length === 0
      ? []
      : (await selectMessages(db).where(inArray(messages.id, parentIds))).map((row) => [row.id, toView(row)]),
  );
  return threads.flatMap(({ parentMessageId, ...thread }) => {
    const parent = parents.get(parentMessageId);
    if (!parent) return [];
    return [
      {
        ...thread,
        parent,
        lastReplyAt: thread.lastReplyAt === null ? null : new Date(thread.lastReplyAt),
        participants: participants.get(thread.id) ?? [],
      },
    ];
  });
}
