import {
  type AgentId,
  REMINDER_MAX_DAYS,
  REMINDER_MAX_SCHEDULED,
  type ReminderRefusal,
  type ReminderRepeat,
  type ReminderView,
  type RoomId,
  reminderRefusalText,
  repeatText,
} from "@crew/protocol";
import { and, asc, count, eq, lte, sql } from "drizzle-orm";
import { notifyMessage } from "./context";
import type { Database } from "./db";
import { reminders, rooms } from "./db/schema";
import { RequestError } from "./errors";
import type { EventHub } from "./events";
import { assertMember, type PostResult, postMessageIn, type Transaction } from "./messages";

// 提醒：Agent 给自己定的一次性或周期提醒。Server 计时，只排下一个到期的；到点在定提醒的房间写一条通知，
// 只唤醒提醒的主人。取舍见 Agent Note：提醒、记忆与静音（2026-10-05-reminders-memory-mute）。

type Row = typeof reminders.$inferSelect;
type Posted = Extract<PostResult, { kind: "posted" }>;

const MINUTE = 60_000;
const DAY = 24 * 60 * MINUTE;

function refuse(refusal: ReminderRefusal): never {
  throw new RequestError(refusal.code === "reminder_not_found" ? 404 : 409, reminderRefusalText(refusal), refusal);
}

/** 本机时区里某一天的 HH:MM。 */
function atClock(day: Date, time: string): Date {
  const [hour = 0, minute = 0] = time.split(":").map(Number);
  const result = new Date(day);
  result.setHours(hour, minute, 0, 0);
  return result;
}

/**
 * 周期规则在 `after` 之后的下一次。`every` 从 `from`（上一次的时间，新建时没有）往后数整数个间隔，
 * 错过的几次跳过，不重放；`daily` 与 `weekly` 按本机时区的钟点，跨夏令时也落在同一个钟点。
 */
export function nextFire(repeat: ReminderRepeat, after: Date, from?: Date): Date {
  switch (repeat.kind) {
    case "every": {
      const step = repeat.minutes * MINUTE;
      if (!from) return new Date(after.getTime() + step);
      const missed = Math.max(0, Math.floor((after.getTime() - from.getTime()) / step));
      return new Date(from.getTime() + (missed + 1) * step);
    }
    case "daily":
    case "weekly": {
      for (let offset = 0; offset <= 7; offset += 1) {
        const day = new Date(after);
        day.setDate(after.getDate() + offset);
        const candidate = atClock(day, repeat.time);
        if (candidate <= after) continue;
        if (repeat.kind === "weekly" && !repeat.days.includes(candidate.getDay())) continue;
        return candidate;
      }
      throw new Error(`周期规则找不到下一次：${JSON.stringify(repeat)}`);
    }
    default: {
      const unknown: never = repeat;
      throw new Error(`未知的周期规则：${JSON.stringify(unknown)}`);
    }
  }
}

function toView(row: Row): ReminderView {
  return {
    id: row.id,
    roomId: row.roomId,
    title: row.title,
    fireAt: row.fireAt.toISOString(),
    repeat: row.repeat ?? null,
    status: row.status,
    createdAt: row.createdAt.toISOString(),
    firedAt: row.firedAt?.toISOString() ?? null,
  };
}

/**
 * Agent 给自己定一个提醒。房间可以是讨论串，成员身份看它所在的群聊。一次性提醒要在将来、一年以内；
 * 周期提醒的第一次按规则从现在算。每个 Agent 同时最多 `REMINDER_MAX_SCHEDULED` 个未触发的。
 */
export async function createReminder(
  db: Database,
  agentId: AgentId,
  now: Date,
  input: { roomId: RoomId; title: string; at?: string; repeat?: ReminderRepeat },
): Promise<ReminderView> {
  return db.transaction(async (tx) => {
    const [room] = await tx.select({ parentRoomId: rooms.parentRoomId }).from(rooms).where(eq(rooms.id, input.roomId));
    if (!room) throw new RequestError(404, "房间不存在");
    await assertMember(tx, room.parentRoomId ?? input.roomId, { kind: "agent", id: agentId });

    // 锁住这个 Agent 的提醒，数量检查与写入之间不会有别的新建插进来。
    await tx.execute(sql`SELECT pg_advisory_xact_lock(hashtext(${`reminders:${agentId}`}))`);
    const [scheduled] = await tx
      .select({ n: count() })
      .from(reminders)
      .where(and(eq(reminders.agentId, agentId), eq(reminders.status, "scheduled")));
    if ((scheduled?.n ?? 0) >= REMINDER_MAX_SCHEDULED) refuse({ code: "reminder_limit", max: REMINDER_MAX_SCHEDULED });

    let fireAt: Date;
    if (input.repeat) {
      fireAt = nextFire(input.repeat, now);
    } else {
      fireAt = new Date(input.at ?? "");
      if (fireAt <= now) refuse({ code: "reminder_past" });
      if (fireAt.getTime() - now.getTime() > REMINDER_MAX_DAYS * DAY) {
        refuse({ code: "reminder_too_far", days: REMINDER_MAX_DAYS });
      }
    }
    const [row] = await tx
      .insert(reminders)
      .values({ agentId, roomId: input.roomId, title: input.title, fireAt, repeat: input.repeat ?? null })
      .returning();
    if (!row) throw new Error("写入提醒失败");
    return toView(row);
  });
}

/** Agent 自己还没触发的提醒，按下一次触发的时间排列。 */
export async function listReminders(db: Database, agentId: AgentId): Promise<ReminderView[]> {
  const rows = await db
    .select()
    .from(reminders)
    .where(and(eq(reminders.agentId, agentId), eq(reminders.status, "scheduled")))
    .orderBy(asc(reminders.fireAt));
  return rows.map(toView);
}

/** 取消自己的一个还没触发的提醒。别人的、已经触发或取消的，都当作不存在。 */
export async function cancelReminder(db: Database, agentId: AgentId, id: string): Promise<ReminderView> {
  const [row] = await db
    .update(reminders)
    .set({ status: "canceled" })
    .where(and(eq(reminders.id, id), eq(reminders.agentId, agentId), eq(reminders.status, "scheduled")))
    .returning();
  if (!row) refuse({ code: "reminder_not_found" });
  return toView(row);
}

const pad = (value: number) => String(value).padStart(2, "0");

/** 通知里的时间：今天写时分，其他日子写月日加时分，按本机时区。 */
function clockText(time: Date, now: Date): string {
  const clock = `${pad(time.getHours())}:${pad(time.getMinutes())}`;
  return time.toDateString() === now.toDateString() ? clock : `${time.getMonth() + 1}月${time.getDate()}日 ${clock}`;
}

/** 晚了这么久才触发（应用当时没在运行），通知里写明原定的时间。 */
const LATE_AFTER_MS = MINUTE;

/** 到点的通知：“的提醒到了：检查 CI（每天 09:00）”，界面显示在作者名字后面。 */
export function reminderNotice(row: Pick<Row, "title" | "repeat" | "fireAt">, now: Date): string {
  const repeat = row.repeat ? `（${repeatText(row.repeat)}）` : "";
  const late =
    now.getTime() - row.fireAt.getTime() > LATE_AFTER_MS
      ? `。原定 ${clockText(row.fireAt, now)}，当时应用没在运行`
      : "";
  return `的提醒到了：${row.title}${repeat}${late}`;
}

/**
 * 触发全部到期的提醒：在定提醒的房间写一条通知（作者是主人，只唤醒主人），一次性的标为已触发，
 * 周期的排到 `now` 之后的下一次，错过的几次不重放。每个提醒单独一个保存点：写通知失败（例如主人已不在群里）
 * 时只取消这一个，不挡住别的。返回写下的通知，调用方在提交后通知界面与 Computer。
 */
export async function fireDueReminders(db: Database, now: Date): Promise<Posted[]> {
  return db.transaction(async (tx) => {
    const due = await tx
      .select()
      .from(reminders)
      .where(and(eq(reminders.status, "scheduled"), lte(reminders.fireAt, now)))
      .orderBy(asc(reminders.fireAt))
      .limit(100)
      .for("update", { skipLocked: true });
    const posted: Posted[] = [];
    for (const row of due) {
      try {
        posted.push(await fireOne(tx, row, now));
      } catch (error) {
        console.error(`[server] 提醒 ${row.id} 触发失败，已取消:`, error);
        await tx.update(reminders).set({ status: "canceled" }).where(eq(reminders.id, row.id));
      }
    }
    return posted;
  });
}

async function fireOne(tx: Transaction, row: Row, now: Date): Promise<Posted> {
  return tx.transaction(async (savepoint) => {
    const result = await postMessageIn(
      savepoint,
      row.roomId,
      { kind: "agent", id: row.agentId },
      reminderNotice(row, now),
      {
        kind: "system",
        notice: {
          type: "reminder",
          title: row.title,
          repeat: row.repeat ?? null,
          setAt: row.createdAt.toISOString(),
          dueAt: row.fireAt.toISOString(),
        },
        wake: [row.agentId],
      },
    );
    if (result.kind !== "posted") throw new Error("提醒的通知被拦下");
    await savepoint
      .update(reminders)
      .set(
        row.repeat
          ? { fireAt: nextFire(row.repeat, now, row.fireAt), firedAt: now }
          : { status: "fired", firedAt: now },
      )
      .where(eq(reminders.id, row.id));
    return result;
  });
}

/** 最早的一个还没触发的提醒的时间；没有时为 undefined。 */
export async function nextDueAt(db: Database): Promise<Date | undefined> {
  const [row] = await db
    .select({ fireAt: reminders.fireAt })
    .from(reminders)
    .where(eq(reminders.status, "scheduled"))
    .orderBy(asc(reminders.fireAt))
    .limit(1);
  return row?.fireAt;
}

/** 计时器最长睡这么久就醒一次：休眠唤醒、改了系统时间之后，不会错过太久。 */
const MAX_SLEEP_MS = 60 * MINUTE;
/** 触发出错后隔这么久再试。 */
const RETRY_MS = MINUTE;

/**
 * 提醒的计时器：只排下一个到期的提醒。启动时先触发应用没运行期间错过的。提醒新建或取消后（`changed`）重新排。
 * 测试不启动它，直接调用 `fireDue`。
 */
export class ReminderScheduler {
  private timer: NodeJS.Timeout | undefined;
  private running = false;
  private again = false;
  private started = false;

  constructor(private readonly deps: { db: Database; events: EventHub; now(): Date }) {}

  start(): void {
    this.started = true;
    this.kick();
  }

  stop(): void {
    this.started = false;
    clearTimeout(this.timer);
  }

  /** 提醒新建或取消了：重新排下一个。没有启动时什么也不做。 */
  changed(): void {
    if (this.started) this.kick();
  }

  /** 触发到期的提醒并通知界面与 Computer。 */
  async fireDue(): Promise<void> {
    for (const post of await fireDueReminders(this.deps.db, this.deps.now())) notifyMessage(this.deps, post);
  }

  private kick(): void {
    if (this.running) {
      this.again = true;
      return;
    }
    void this.run();
  }

  private async run(): Promise<void> {
    this.running = true;
    clearTimeout(this.timer);
    let delay: number | undefined;
    try {
      // 查下一个到期时间期间也可能有新建或取消：重来一遍，否则按旧的结果排，新提醒要等下一次变化才触发。
      let next: Date | undefined;
      do {
        this.again = false;
        await this.fireDue();
        next = await nextDueAt(this.deps.db);
      } while (this.again);
      if (next) delay = Math.min(Math.max(0, next.getTime() - this.deps.now().getTime()), MAX_SLEEP_MS);
    } catch (error) {
      console.error("[server] 提醒计时器出错，稍后重试:", error);
      delay = RETRY_MS;
    } finally {
      this.running = false;
    }
    if (this.started && delay !== undefined) this.timer = setTimeout(() => this.kick(), delay);
  }
}
