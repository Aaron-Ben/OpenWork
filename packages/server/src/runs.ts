import {
  type AgentId,
  type AgentStatus,
  clipText,
  type EngineEvent,
  type MessageId,
  type RoomId,
  type RunDetail,
  type RunEvent,
  type RunSummary,
  type RunTrigger,
  storableText,
} from "@crew/protocol";
import { and, asc, desc, eq, gt, inArray, sql } from "drizzle-orm";
import type { Database } from "./db";
import { messages, runEvents, runs, runTriggers } from "./db/schema";
import { RequestError } from "./errors";

// 运行记录：Computer 登记一轮、追加 Engine 事件、写结果；Server 自己记下这一轮里的回复与 HELD。
// Agent 的状态由这里推出，见 `agentStatuses`。

type Transaction = Parameters<Parameters<Database["transaction"]>[0]>[0];
type Run = typeof runs.$inferSelect;

/** 事件里的文字字段写入前换掉 PostgreSQL 不接受的字符，见 protocol 的 `storableText`。 */
function storableData(data: Record<string, unknown>): Record<string, unknown> {
  return Object.fromEntries(
    Object.entries(data).map(([key, value]) => [key, typeof value === "string" ? storableText(value) : value]),
  );
}

/** 一轮有了变化：通知界面时带上这一轮与它涉及的房间。 */
export interface RunActivity {
  runId: string;
  roomIds: RoomId[];
}

/** HELD 事件里新消息预览的长度。 */
const PREVIEW_MAX = 120;

/**
 * 开始一轮。这个 Agent 还有没结束的一轮时（上一个 Computer 异常退出、还没重连），先把它标为中断：
 * 同一个 Agent 同一时间最多一轮在跑，数据库的唯一索引也这样要求。
 */
export async function startRun(
  db: Database,
  agentId: AgentId,
  input: { prompt: string; triggers: RunTrigger[] },
): Promise<{ id: string; roomIds: RoomId[] }> {
  return db.transaction(async (tx) => {
    await tx
      .update(runs)
      .set({ outcome: "interrupted", endedAt: sql`now()` })
      .where(and(eq(runs.agentId, agentId), eq(runs.outcome, "running")));
    const [run] = await tx
      .insert(runs)
      .values({ agentId, prompt: storableText(input.prompt) })
      .returning({ id: runs.id });
    if (!run) throw new Error("登记一轮失败");
    await tx.insert(runTriggers).values(input.triggers.map((trigger) => ({ runId: run.id, ...trigger })));
    return { id: run.id, roomIds: input.triggers.map((trigger) => trigger.roomId) };
  });
}

/** 锁住进行中的一轮。不存在时 404，已经结束时 409：上报晚到了，这一轮已被标为中断或已写结果。 */
async function lockRunning(tx: Transaction, runId: string): Promise<Run> {
  const [run] = await tx.select().from(runs).where(eq(runs.id, runId)).for("update");
  if (!run) throw new RequestError(404, "这一轮不存在");
  if (run.outcome !== "running") throw new RequestError(409, "这一轮已经结束");
  return run;
}

/** 还没有序号的一步：Engine 事件，或 Server 记下的回复与 HELD。 */
type NewEvent =
  | EngineEvent
  | { kind: "reply"; at: string; roomId: RoomId; messageId: MessageId; body: string }
  | { kind: "held"; at: string; roomId: RoomId; newMessages: number; preview: string };

/** 把事件写进一轮，序号接着上一个。调用方已锁住这一轮。 */
async function insertEvents(tx: Transaction, run: Run, events: NewEvent[]): Promise<void> {
  const rows = events.map((event, index) => {
    const { kind, at, ...data } = event;
    return { runId: run.id, seq: run.lastEventSeq + index + 1, kind, at: new Date(at), data: storableData(data) };
  });
  await tx.insert(runEvents).values(rows);
  await tx
    .update(runs)
    .set({ lastEventSeq: run.lastEventSeq + rows.length })
    .where(eq(runs.id, run.id));
}

/** 追加 Engine 事件。每一步结束时把用量累加到这一轮上。 */
export async function appendEngineEvents(db: Database, runId: string, events: EngineEvent[]): Promise<RoomId[]> {
  return db.transaction(async (tx) => {
    const run = await lockRunning(tx, runId);
    await insertEvents(tx, run, events);
    const ends = events.flatMap((event) => (event.kind === "step_end" ? [event.usage] : []));
    const sum = (pick: (usage: (typeof ends)[number]) => number) => ends.reduce((total, u) => total + pick(u), 0);
    await tx
      .update(runs)
      .set({
        steps: sql`${runs.steps} + ${events.filter((event) => event.kind === "step").length}`,
        inputTokens: sql`${runs.inputTokens} + ${sum((u) => u.input)}`,
        outputTokens: sql`${runs.outputTokens} + ${sum((u) => u.output)}`,
        reasoningTokens: sql`${runs.reasoningTokens} + ${sum((u) => u.reasoning)}`,
        cacheReadTokens: sql`${runs.cacheReadTokens} + ${sum((u) => u.cacheRead)}`,
        cacheWriteTokens: sql`${runs.cacheWriteTokens} + ${sum((u) => u.cacheWrite)}`,
        cost: sql`${runs.cost} + ${sum((u) => u.cost)}`,
      })
      .where(eq(runs.id, runId));
    return triggerRooms(tx, runId);
  });
}

/** 一轮结束。已经结束的（例如被标为中断）返回 409。 */
export async function finishRun(
  db: Database,
  runId: string,
  result: { outcome: "succeeded" | "cancelled" } | { outcome: "failed"; error: string },
): Promise<{ agentId: AgentId; roomIds: RoomId[] }> {
  return db.transaction(async (tx) => {
    const run = await lockRunning(tx, runId);
    await tx
      .update(runs)
      .set({ outcome: result.outcome, error: result.outcome === "failed" ? result.error : null, endedAt: sql`now()` })
      .where(eq(runs.id, runId));
    return { agentId: run.agentId, roomIds: await triggerRooms(tx, runId) };
  });
}

/** 新的 Computer 连上时调用：上一个 Computer 没结束的轮次都不会再有结果，标为中断。 */
export async function interruptRunning(db: Database): Promise<string[]> {
  const rows = await db
    .update(runs)
    .set({ outcome: "interrupted", endedAt: sql`now()` })
    .where(eq(runs.outcome, "running"))
    .returning({ id: runs.id });
  return rows.map((row) => row.id);
}

async function runningRunOf(tx: Transaction, agentId: AgentId): Promise<Run | undefined> {
  const [run] = await tx
    .select()
    .from(runs)
    .where(and(eq(runs.agentId, agentId), eq(runs.outcome, "running")))
    .for("update");
  return run;
}

/**
 * Agent 发出了一条回复：记进它正在跑的那一轮，并在消息上记下所在的轮次与发出前被 HELD 拦下的次数
 * （这一轮里、这个房间、上一条回复之后的 HELD）。不在任何一轮里（例如测试直接调用接口）时什么也不记。
 */
export async function recordReply(
  db: Database,
  agentId: AgentId,
  reply: { roomId: RoomId; messageId: MessageId; body: string },
): Promise<RunActivity | undefined> {
  return db.transaction(async (tx) => {
    const run = await runningRunOf(tx, agentId);
    if (!run) return undefined;
    const [lastReply] = await tx
      .select({ seq: sql<number>`coalesce(max(${runEvents.seq}), 0)` })
      .from(runEvents)
      .where(and(eq(runEvents.runId, run.id), eq(runEvents.kind, "reply"), roomIs(reply.roomId)));
    const [held] = await tx
      .select({ n: sql<number>`count(*)::int` })
      .from(runEvents)
      .where(
        and(
          eq(runEvents.runId, run.id),
          eq(runEvents.kind, "held"),
          roomIs(reply.roomId),
          gt(runEvents.seq, lastReply?.seq ?? 0),
        ),
      );
    await insertEvents(tx, run, [{ kind: "reply", at: new Date().toISOString(), ...reply }]);
    await tx
      .update(runs)
      .set({ replies: sql`${runs.replies} + 1` })
      .where(eq(runs.id, run.id));
    await tx
      .update(messages)
      .set({ runId: run.id, heldBefore: held?.n ?? 0 })
      .where(eq(messages.id, reply.messageId));
    return { runId: run.id, roomIds: await triggerRooms(tx, run.id) };
  });
}

/** Agent 的回复被 HELD 拦下：记进它正在跑的那一轮。 */
export async function recordHeld(
  db: Database,
  agentId: AgentId,
  held: { roomId: RoomId; newMessages: number; preview: string },
): Promise<RunActivity | undefined> {
  return db.transaction(async (tx) => {
    const run = await runningRunOf(tx, agentId);
    if (!run) return undefined;
    await insertEvents(tx, run, [
      { kind: "held", at: new Date().toISOString(), ...held, preview: clipText(held.preview, PREVIEW_MAX) },
    ]);
    await tx
      .update(runs)
      .set({ holds: sql`${runs.holds} + 1` })
      .where(eq(runs.id, run.id));
    return { runId: run.id, roomIds: await triggerRooms(tx, run.id) };
  });
}

const roomIs = (roomId: RoomId) => sql`${runEvents.data}->>'roomId' = ${roomId}`;

async function triggerRooms(tx: Transaction | Database, runId: string): Promise<RoomId[]> {
  const rows = await tx.select({ roomId: runTriggers.roomId }).from(runTriggers).where(eq(runTriggers.runId, runId));
  return rows.map((row) => row.roomId);
}

async function triggersOf(db: Database, runIds: string[]): Promise<Map<string, RunTrigger[]>> {
  const result = new Map<string, RunTrigger[]>();
  if (runIds.length === 0) return result;
  const rows = await db.select().from(runTriggers).where(inArray(runTriggers.runId, runIds));
  for (const { runId, ...trigger } of rows) result.set(runId, [...(result.get(runId) ?? []), trigger]);
  return result;
}

function summary(run: Run, triggers: RunTrigger[]): RunSummary {
  return {
    id: run.id,
    agentId: run.agentId,
    outcome: run.outcome,
    error: run.error,
    startedAt: run.startedAt.toISOString(),
    endedAt: run.endedAt?.toISOString() ?? null,
    triggers,
    usage: {
      input: run.inputTokens,
      output: run.outputTokens,
      reasoning: run.reasoningTokens,
      cacheRead: run.cacheReadTokens,
      cacheWrite: run.cacheWriteTokens,
      cost: run.cost,
    },
    steps: run.steps,
    replies: run.replies,
    holds: run.holds,
  };
}

/** 列表上限。更早的轮次以后需要时再分页。 */
export const RUN_LIST_MAX = 100;

/** 运行记录，从新到旧。给了房间时只列被这个房间唤醒的轮次。 */
export async function listRuns(db: Database, filter: { roomId?: RoomId; agentId?: AgentId }): Promise<RunSummary[]> {
  const rows = await db
    .select()
    .from(runs)
    .where(
      and(
        filter.agentId ? eq(runs.agentId, filter.agentId) : undefined,
        filter.roomId
          ? inArray(
              runs.id,
              db.select({ id: runTriggers.runId }).from(runTriggers).where(eq(runTriggers.roomId, filter.roomId)),
            )
          : undefined,
      ),
    )
    .orderBy(desc(runs.startedAt))
    .limit(RUN_LIST_MAX);
  const triggers = await triggersOf(
    db,
    rows.map((run) => run.id),
  );
  return rows.map((run) => summary(run, triggers.get(run.id) ?? []));
}

/** @throws RequestError 404：这一轮不存在。 */
export async function getRun(db: Database, runId: string): Promise<RunDetail> {
  const [run] = await db.select().from(runs).where(eq(runs.id, runId));
  if (!run) throw new RequestError(404, "这一轮不存在");
  const rows = await db.select().from(runEvents).where(eq(runEvents.runId, runId)).orderBy(asc(runEvents.seq));
  const triggers = await triggersOf(db, [runId]);
  const events = rows.map((row) => ({
    seq: row.seq,
    kind: row.kind,
    at: row.at.toISOString(),
    ...(row.data as object),
  }));
  return { ...summary(run, triggers.get(runId) ?? []), prompt: run.prompt, events: events as RunEvent[] };
}

/**
 * 每个 Agent 的状态：有进行中的一轮是回复中；否则 Computer 报告的问题优先；否则看最近结束的一轮，失败就是出错。
 * 列表里没有的 Agent 是空闲。
 */
export async function agentStatuses(
  db: Database,
  problems: ReadonlyMap<AgentId, string>,
): Promise<Map<AgentId, AgentStatus>> {
  const running = await db.select().from(runs).where(eq(runs.outcome, "running"));
  const latest = await db
    .selectDistinctOn([runs.agentId])
    .from(runs)
    .where(sql`${runs.outcome} <> 'running'`)
    .orderBy(runs.agentId, desc(runs.startedAt));
  const triggers = await triggersOf(
    db,
    [...running, ...latest].map((run) => run.id),
  );
  const rooms = (runId: string) => (triggers.get(runId) ?? []).map((trigger) => trigger.roomId);

  const statuses = new Map<AgentId, AgentStatus>();
  for (const run of latest) {
    if (run.outcome === "failed" && run.error) {
      statuses.set(run.agentId, { state: "error", reason: run.error, roomIds: rooms(run.id) });
    }
  }
  for (const [agentId, reason] of problems) statuses.set(agentId, { state: "error", reason, roomIds: [] });
  for (const run of running) statuses.set(run.agentId, { state: "working", runId: run.id, roomIds: rooms(run.id) });
  return statuses;
}
