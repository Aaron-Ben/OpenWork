import {
  assertNever,
  MessageId,
  RoomId,
  TASK_STATUSES,
  TASK_TITLE_MAX,
  TASK_TRANSITIONS,
  TaskRefusal,
  TaskStatus,
  TaskView,
} from "@crew/protocol";
import type { Command } from "commander";
import { z } from "zod";
import { CliFailure, type CliIo, errorBody, postAgent } from "./io";

// `crew task`：Agent 新建、转换、领取、改状态、分配任务。输出的每一行都会被模型读到，
// 由快照 test/__snapshots__/shim-output.md 逐字锁定。

const UNSURE = "The change may have been made; run crew task list to check before trying again.";

export function registerTaskCommands(program: Command, io: CliIo): void {
  const task = program
    .command("task")
    .description("Create, claim and update tasks in a room.")
    .helpCommand(false)
    .addHelpText("after", `\nStatuses: ${TASK_STATUSES.join(", ")}. The room id can also be a task's thread id.\n`);

  task
    .command("list")
    .description("List the tasks in a room.")
    .argument("<room-id>", "the room, as shown above your unread messages")
    .action(async (roomArg: string) => {
      const roomId = parseRoom(roomArg);
      const tasks = z.array(TaskView).parse(await call(io, "/agent/tasks/list", { roomId }, roomId));
      io.stdout(listText(roomId, tasks));
    });

  task
    .command("create")
    .description("Create a task: posts the title as a message in the room and turns it into a task.")
    .argument("<room-id>", "the room to create it in")
    .argument("<title>", `what needs doing, in one line (at most ${TASK_TITLE_MAX} characters)`)
    .option("--assign <handle>", "assign it to an agent in the room")
    .action(async (roomArg: string, title: string, options: { assign?: string }) => {
      const roomId = parseRoom(roomArg);
      const body = { roomId, title, assign: options.assign && parseHandle(options.assign) };
      const created = TaskView.parse(await call(io, "/agent/tasks/create", body, roomId));
      io.stdout(`Created ${describe(created)}.${where(created)}\n`);
    });

  task
    .command("convert")
    .description("Turn a message in the room into a task. Its first line becomes the title.")
    .argument("<room-id>", "the room the message is in")
    .argument("<message-id>", "the id shown in brackets before the message")
    .option("--assign <handle>", "assign it to an agent in the room")
    .action(async (roomArg: string, messageArg: string, options: { assign?: string }) => {
      const roomId = parseRoom(roomArg);
      const messageId = MessageId.safeParse(messageArg);
      if (!messageId.success) {
        throw new CliFailure(`"${messageArg}" is not a message id. Use the id shown in brackets before a message.`);
      }
      const body = { roomId, messageId: messageId.data, assign: options.assign && parseHandle(options.assign) };
      const created = TaskView.parse(await call(io, "/agent/tasks/convert", body, roomId, messageId.data));
      io.stdout(`Created ${describe(created)}.${where(created)}\n`);
    });

  task
    .command("claim")
    .description("Take a todo task: you become its assignee and it moves to in_progress.")
    .argument("<room-id>", "the room the task is in")
    .argument("<number>", "the task number, as in #3")
    .action(async (roomArg: string, numberArg: string) => {
      const roomId = parseRoom(roomArg);
      const body = { roomId, number: parseNumber(numberArg) };
      const claimed = TaskView.parse(await call(io, "/agent/tasks/claim", body, roomId));
      io.stdout(`You have ${describe(claimed)}.${where(claimed)} When the work is done, set it to in_review.\n`);
    });

  task
    .command("status")
    .description("Change a task's status.")
    .argument("<room-id>", "the room the task is in")
    .argument("<number>", "the task number, as in #3")
    .argument("<status>", TASK_STATUSES.join(" | "))
    .action(async (roomArg: string, numberArg: string, statusArg: string) => {
      const roomId = parseRoom(roomArg);
      const status = TaskStatus.safeParse(statusArg);
      if (!status.success) {
        throw new CliFailure(`"${statusArg}" is not a status. Use one of: ${TASK_STATUSES.join(", ")}.`);
      }
      const body = { roomId, number: parseNumber(numberArg), status: status.data };
      const updated = TaskView.parse(await call(io, "/agent/tasks/status", body, roomId));
      io.stdout(`Updated ${describe(updated)}.\n`);
    });

  task
    .command("assign")
    .description("Assign a task to an agent in the room. It stays todo until that agent claims it.")
    .argument("<room-id>", "the room the task is in")
    .argument("<number>", "the task number, as in #3")
    .argument("<handle>", "the agent's @handle")
    .action(async (roomArg: string, numberArg: string, handleArg: string) => {
      const roomId = parseRoom(roomArg);
      const body = { roomId, number: parseNumber(numberArg), assign: parseHandle(handleArg) };
      const updated = TaskView.parse(await call(io, "/agent/tasks/assign", body, roomId));
      io.stdout(`Updated ${describe(updated)}.\n`);
    });
}

function parseRoom(arg: string): RoomId {
  const roomId = RoomId.safeParse(arg);
  if (!roomId.success) throw new CliFailure(`"${arg}" is not a room id. Use the id shown above your unread messages.`);
  return roomId.data;
}

function parseNumber(arg: string): number {
  const number = Number(arg.replace(/^#/, ""));
  if (!Number.isInteger(number) || number <= 0)
    throw new CliFailure(`"${arg}" is not a task number. Use the number, as in 3.`);
  return number;
}

/** handle 可以带或不带 `@`。 */
function parseHandle(arg: string): string {
  return arg.replace(/^@/, "").toLowerCase();
}

/** 调用一个任务接口，成功时返回响应正文；被拒绝时换成英文说明。 */
async function call(io: CliIo, path: string, body: unknown, roomId: RoomId, messageId?: MessageId): Promise<unknown> {
  const response = await postAgent(io, path, body, UNSURE);
  if (response.status === 200) return response.json();
  const error = await errorBody(response);
  const refusal = TaskRefusal.safeParse(error?.refusal);
  if (refusal.success) throw new CliFailure(refusalText(refusal.data, roomId, messageId));
  switch (response.status) {
    case 401:
      throw new CliFailure("Crew rejected your token. Crew may have restarted; nothing was changed.");
    case 403:
      throw new CliFailure(`you are not a member of room ${roomId}. Use the rooms listed in your turn.`);
    case 404:
      throw new CliFailure(`room ${roomId} does not exist. Use the rooms listed in your turn.`);
    default:
      throw new CliFailure(
        `Crew refused the change (HTTP ${response.status}${error ? `: ${error.error}` : ""}). Nothing was changed.`,
      );
  }
}

/** Server 的拒绝原因写给 Agent 的英文说明：发生了什么，下一步做什么。 */
function refusalText(refusal: TaskRefusal, roomId: RoomId, messageId: MessageId | undefined): string {
  switch (refusal.code) {
    case "not_found":
      return `there is no task #${refusal.number} in room ${roomId}. Run crew task list ${roomId} to see its tasks.`;
    case "message_not_found":
      return `message ${messageId ?? "?"} is not in room ${roomId}.`;
    case "system_message":
      return `message ${messageId ?? "?"} is a notice, and notices can't become tasks.`;
    case "in_thread":
      return "messages inside a thread can't become tasks. Create a task in the room with crew task create instead.";
    case "already_task":
      return `that message is already task #${refusal.number}.`;
    case "not_member":
      return `@${refusal.handle} is not an agent in room ${roomId}.`;
    case "claimed":
      return `task #${refusal.number} is already taken by @${refusal.by}. Don't start work on it.`;
    case "not_claimable":
      return `task #${refusal.number} is ${refusal.status}; only todo tasks can be claimed.`;
    case "transition":
      return `task #${refusal.number} can't go from ${refusal.from} to ${refusal.to}. From ${refusal.from} it can go to: ${TASK_TRANSITIONS[refusal.from].join(", ")}.`;
    case "needs_assignee":
      return `task #${refusal.number} has no assignee yet. Claim it with crew task claim ${roomId} ${refusal.number} first.`;
    case "finished":
      return `task #${refusal.number} is ${refusal.status} and can't be reassigned.`;
    case "changed":
      return `task #${refusal.number} was just changed by someone else. Run crew task list ${roomId} and decide again.`;
    default:
      return assertNever(refusal);
  }
}

/** `task #3 "标题" (in_progress, assigned to @alice)` */
function describe(task: TaskView): string {
  const who = task.assignee ? `assigned to @${task.assignee.handle}` : "unassigned";
  return `task #${task.number} "${task.title}" (${task.status}, ${who})`;
}

/** 在哪里汇报进展：群聊里是任务的讨论串，私聊里是私聊本身。 */
function where(task: TaskView): string {
  return task.threadId
    ? ` Post updates in its thread: crew reply ${task.threadId}.`
    : ` Post updates in room ${task.roomId}.`;
}

function listText(roomId: RoomId, tasks: TaskView[]): string {
  if (tasks.length === 0) return `No tasks in room ${roomId} yet.\n`;
  const lines = tasks.map((task) => {
    const who = task.assignee ? `@${task.assignee.handle}` : "unassigned";
    const thread = task.threadId ? `, thread ${task.threadId}` : "";
    return `  #${task.number} [${task.status}] ${task.title} (${who}${thread})`;
  });
  return `Tasks in room ${roomId}:\n${lines.join("\n")}\n`;
}
