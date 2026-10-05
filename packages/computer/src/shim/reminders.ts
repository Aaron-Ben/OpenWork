import {
  assertNever,
  REMINDER_MAX_DAYS,
  REMINDER_MIN_MINUTES,
  ReminderRefusal,
  type ReminderRepeat,
  ReminderView,
  RoomId,
} from "@crew/protocol";
import type { Command } from "commander";
import { z } from "zod";
import { localTimestamp } from "../prompt";
import { CliFailure, type CliIo, errorBody, postAgent } from "./io";

// `crew remind`：Agent 给自己定提醒，到点在定它的房间里收到一条通知并被唤醒。输出的每一行都会被模型读到，
// 由快照 test/__snapshots__/shim-output.md 逐字锁定。时间按本机时区。

const UNSURE = "The reminder may have been changed; run crew remind list to check before trying again.";

const WEEKDAYS = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"] as const;
/** 提醒最远定到这么多分钟以后，与 Server 的上限一致；更大的数在本地就拒绝，免得算出无效的时间。 */
const MAX_MINUTES = REMINDER_MAX_DAYS * 24 * 60;

interface WhenOptions {
  in?: string;
  at?: string;
  every?: string;
  daily?: string;
  weekly?: string;
}

export function registerReminderCommands(program: Command, io: CliIo): void {
  const remind = program
    .command("remind")
    .description("Set reminders that wake you later. Nothing else wakes you unless a message arrives.")
    .helpCommand(false);

  remind
    .command("set", { isDefault: true })
    .description("Set a reminder: when it is due you get a notice in the room and wake up there.")
    .argument("<room-id>", "the room (or thread) to be reminded in")
    .argument("<title>", "what to do then, in one line")
    .option("--in <duration>", "once, after a while: 30m, 2h, 1d")
    .option("--at <time>", "once, at a local time: 18:00 (the next one), 2026-10-06 09:00, or ISO 8601")
    .option("--every <duration>", `repeatedly, every 30m, 2h, ... (at least ${REMINDER_MIN_MINUTES}m)`)
    .option("--daily <HH:MM>", "every day at this local time")
    .option("--weekly <days@HH:MM>", "on these days at this local time: mon,fri@09:00")
    .addHelpText("after", `\nExample:\n  crew remind <room-id> "Check whether CI passed" --in 30m\n`)
    .action(async (roomArg: string, title: string, options: WhenOptions) => {
      const roomId = parseRoom(roomArg);
      const now = new Date();
      const when = parseWhen(options, now);
      const created = ReminderView.parse(await call(io, "/agent/reminders/create", { roomId, title, ...when }));
      io.stdout(
        `Reminder set: ${describe(created)}. When it is due you get a notice in ${roomId} and wake up there.\n`,
      );
    });

  remind
    .command("list")
    .description("List your reminders that have not fired yet.")
    .action(async () => {
      const list = z.array(ReminderView).parse(await call(io, "/agent/reminders/list", undefined));
      io.stdout(
        list.length === 0
          ? "You have no reminders.\n"
          : `Your reminders:\n${list.map((reminder) => `  ${reminder.id}  ${describe(reminder)}, in ${reminder.roomId}`).join("\n")}\n`,
      );
    });

  remind
    .command("cancel")
    .description("Cancel one of your reminders.")
    .argument("<id>", "the reminder id, as shown by crew remind list")
    .action(async (idArg: string) => {
      const id = z.uuid().safeParse(idArg);
      if (!id.success) throw new CliFailure(`"${idArg}" is not a reminder id. Run crew remind list to see the ids.`);
      const canceled = ReminderView.parse(await call(io, "/agent/reminders/cancel", { id: id.data }));
      io.stdout(`Canceled: ${describe(canceled)}.\n`);
    });
}

export function parseRoom(arg: string): RoomId {
  const roomId = RoomId.safeParse(arg);
  if (!roomId.success) throw new CliFailure(`"${arg}" is not a room id. Use the id shown above your unread messages.`);
  return roomId.data;
}

/** `30m`、`2h`、`1d` 换成分钟。 */
export function parseDuration(text: string, flag: string): number {
  const match = /^(\d+)\s*(m|min|h|d)$/.exec(text.trim());
  if (!match) throw new CliFailure(`${flag} "${text}" is not a duration. Write it like 30m, 2h or 1d.`);
  const amount = Number(match[1]);
  const unit = match[2] === "d" ? 24 * 60 : match[2] === "h" ? 60 : 1;
  return amount * unit;
}

function parseClock(text: string, flag: string): string {
  const match = /^(\d{1,2}):(\d{2})$/.exec(text.trim());
  if (!match || Number(match[1]) > 23 || Number(match[2]) > 59) {
    throw new CliFailure(`${flag} "${text}" is not a time of day. Write it like 09:00 or 18:30.`);
  }
  return `${match[1]?.padStart(2, "0")}:${match[2]}`;
}

/**
 * 日历上真有这一天、这一刻。`Date` 会把 2 月 30 日、18:99 悄悄顺延成别的时间，提醒就会在 Agent 没想到的时候响，
 * 所以先检查各部分的范围。
 */
function realMoment(year: number, month: number, day: number, hour: number, minute: number): boolean {
  const daysInMonth = new Date(Date.UTC(year, month, 0)).getUTCDate();
  return month >= 1 && month <= 12 && day >= 1 && day <= daysInMonth && hour <= 23 && minute <= 59;
}

/** 一次性提醒的时间：`18:00` 是下一个 18:00（今天还没到就是今天），也接受本地日期时间与带时区的 ISO 8601。 */
function parseAt(text: string, now: Date): Date {
  const value = text.trim();
  const fail = () =>
    new CliFailure(`--at "${text}" is not a time. Write it like 18:00, 2026-10-06 09:00, or ISO 8601 with an offset.`);
  const clock = /^(\d{1,2}):(\d{2})$/.exec(value);
  if (clock) {
    const [hour, minute] = [Number(clock[1]), Number(clock[2])];
    if (!realMoment(2000, 1, 1, hour, minute)) throw fail();
    const at = new Date(now);
    at.setHours(hour, minute, 0, 0);
    if (at <= now) at.setDate(at.getDate() + 1);
    return at;
  }
  const local = /^(\d{4})-(\d{2})-(\d{2})[ T](\d{1,2}):(\d{2})$/.exec(value);
  if (local) {
    const [year, month, day, hour, minute] = local.slice(1).map(Number) as [number, number, number, number, number];
    if (!realMoment(year, month, day, hour, minute)) throw fail();
    return new Date(year, month - 1, day, hour, minute);
  }
  const iso = /^(\d{4})-(\d{2})-(\d{2})[Tt ](\d{2}):(\d{2})(?::\d{2}(?:\.\d+)?)?(?:[zZ]|[+-]\d{2}:?\d{2})$/.exec(value);
  if (iso) {
    const [year, month, day, hour, minute] = iso.slice(1, 6).map(Number) as [number, number, number, number, number];
    const at = new Date(value);
    if (realMoment(year, month, day, hour, minute) && !Number.isNaN(at.getTime())) return at;
  }
  throw fail();
}

/** 恰好一个时间选项，换成接口要的 `at` 或 `repeat`。 */
function parseWhen(options: WhenOptions, now: Date): { at: string } | { repeat: ReminderRepeat } {
  const given = (["in", "at", "every", "daily", "weekly"] as const).filter((key) => options[key] !== undefined);
  if (given.length !== 1) {
    throw new CliFailure("give exactly one of --in, --at, --every, --daily or --weekly. Run crew remind set --help.");
  }
  if (options.in !== undefined) {
    const minutes = parseDuration(options.in, "--in");
    if (minutes > MAX_MINUTES) throw new CliFailure(`--in can be at most ${REMINDER_MAX_DAYS}d.`);
    return { at: new Date(now.getTime() + minutes * 60_000).toISOString() };
  }
  if (options.at !== undefined) return { at: parseAt(options.at, now).toISOString() };
  if (options.every !== undefined) {
    const minutes = parseDuration(options.every, "--every");
    if (minutes > MAX_MINUTES) throw new CliFailure(`--every can be at most ${REMINDER_MAX_DAYS}d.`);
    if (minutes < REMINDER_MIN_MINUTES) {
      throw new CliFailure(
        `--every must be at least ${REMINDER_MIN_MINUTES}m, so a reminder does not wake you too often.`,
      );
    }
    return { repeat: { kind: "every", minutes } };
  }
  if (options.daily !== undefined) return { repeat: { kind: "daily", time: parseClock(options.daily, "--daily") } };
  const [daysText = "", clockText = ""] = (options.weekly ?? "").split("@");
  const days = daysText
    .split(",")
    .map((day) => WEEKDAYS.indexOf(day.trim().toLowerCase().slice(0, 3) as (typeof WEEKDAYS)[number]));
  if (days.length === 0 || days.some((day) => day < 0) || !clockText) {
    throw new CliFailure(`--weekly "${options.weekly}" is not a weekly time. Write it like mon,fri@09:00.`);
  }
  return { repeat: { kind: "weekly", days: [...new Set(days)], time: parseClock(clockText, "--weekly") } };
}

function repeatText(repeat: ReminderRepeat): string {
  switch (repeat.kind) {
    case "every":
      return repeat.minutes % 60 === 0 ? `every ${repeat.minutes / 60}h` : `every ${repeat.minutes}m`;
    case "daily":
      return `daily at ${repeat.time}`;
    case "weekly":
      return `every ${[...repeat.days]
        .sort((a, b) => a - b)
        .map((day) => WEEKDAYS[day])
        .join(",")} at ${repeat.time}`;
    default:
      return assertNever(repeat);
  }
}

/** `"标题" at 2026-10-06T09:00:00+08:00 (daily at 09:00)` */
function describe(reminder: ReminderView): string {
  const repeat = reminder.repeat ? ` (${repeatText(reminder.repeat)})` : "";
  const when = reminder.repeat ? "next at" : "at";
  return `"${reminder.title}" ${when} ${localTimestamp(new Date(reminder.fireAt))}${repeat}`;
}

async function call(io: CliIo, path: string, body: unknown): Promise<unknown> {
  const response = await postAgent(io, path, body, UNSURE);
  if (response.status === 200) return response.json();
  const error = await errorBody(response);
  const refusal = ReminderRefusal.safeParse(error?.refusal);
  if (refusal.success) throw new CliFailure(refusalText(refusal.data));
  switch (response.status) {
    case 401:
      throw new CliFailure("Crew rejected your token. Crew may have restarted; nothing was changed.");
    case 403:
      throw new CliFailure("you are not a member of that room. Use the rooms listed in your turn.");
    case 404:
      throw new CliFailure("that room does not exist. Use the rooms listed in your turn.");
    default:
      throw new CliFailure(
        `Crew refused the reminder (HTTP ${response.status}${error ? `: ${error.error}` : ""}). Nothing was changed.`,
      );
  }
}

function refusalText(refusal: ReminderRefusal): string {
  switch (refusal.code) {
    case "reminder_limit":
      return `you already have ${refusal.max} reminders waiting. Cancel ones you no longer need (crew remind list, crew remind cancel <id>).`;
    case "reminder_past":
      return "that time has already passed. Give a time in the future.";
    case "reminder_too_far":
      return `a reminder can be at most ${REMINDER_MAX_DAYS} days ahead.`;
    case "reminder_not_found":
      return "you have no waiting reminder with that id. Run crew remind list to see your reminders.";
    default:
      return assertNever(refusal);
  }
}
