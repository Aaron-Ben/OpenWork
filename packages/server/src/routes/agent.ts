import { api } from "@crew/protocol";
import type { Express } from "express";
import { notifyMessage, notifyRun, notifyTaskChange, type ServerContext } from "../context";
import { agentOf, route } from "../http";
import { postMessage } from "../messages";
import { cancelReminder, createReminder, listReminders } from "../reminders";
import { recordHeld, recordReply } from "../runs";
import { assignTask, claimTask, convertToTask, createTask, listTasks, setTaskStatus } from "../tasks";

/** Agent 经 `crew` 命令调用的接口。凭证决定是哪个 Agent，请求体里不能指定身份。 */
export function agentRoutes(app: Express, ctx: ServerContext): void {
  const self = (response: Parameters<typeof agentOf>[0]) => ({ kind: "agent" as const, id: agentOf(response) });

  route(app, api.agent.createReminder, async ({ body, response }) => {
    const reminder = await createReminder(ctx.db, agentOf(response), ctx.now(), body);
    ctx.reminders.changed();
    return reminder;
  });

  route(app, api.agent.listReminders, ({ response }) => listReminders(ctx.db, agentOf(response)));

  route(app, api.agent.cancelReminder, async ({ body, response }) => {
    const reminder = await cancelReminder(ctx.db, agentOf(response), body.id);
    ctx.reminders.changed();
    return reminder;
  });

  route(app, api.agent.listTasks, ({ body, response }) => listTasks(ctx.db, self(response), body.roomId));

  route(app, api.agent.createTask, async ({ body, response }) =>
    notifyTaskChange(
      ctx,
      await createTask(ctx.db, self(response), body.roomId, {
        title: body.title,
        assignee: body.assign ? { handle: body.assign } : undefined,
      }),
    ),
  );

  route(app, api.agent.convertToTask, async ({ body, response }) =>
    notifyTaskChange(
      ctx,
      await convertToTask(ctx.db, self(response), body.roomId, {
        messageId: body.messageId,
        assignee: body.assign ? { handle: body.assign } : undefined,
      }),
    ),
  );

  route(app, api.agent.claimTask, async ({ body, response }) =>
    notifyTaskChange(ctx, await claimTask(ctx.db, agentOf(response), body.roomId, body.number)),
  );

  route(app, api.agent.setTaskStatus, async ({ body, response }) =>
    notifyTaskChange(
      ctx,
      await setTaskStatus(ctx.db, self(response), body.roomId, body.number, body.status, body.note),
    ),
  );

  route(app, api.agent.assignTask, async ({ body, response }) =>
    notifyTaskChange(ctx, await assignTask(ctx.db, self(response), body.roomId, body.number, { handle: body.assign })),
  );

  route(app, api.agent.reply, async ({ body, response }) => {
    const agentId = agentOf(response);
    const result = await postMessage(ctx.db, body.roomId, { kind: "agent", id: agentId }, body.body, body.threadOf);
    if (result.kind === "held") {
      const newMessages = result.newMessages.length + result.omitted;
      const preview = result.newMessages.map((message) => message.body).join(" / ");
      notifyRun(ctx, await observe(recordHeld(ctx.db, agentId, { roomId: result.roomId, newMessages, preview })));
      return {
        outcome: "held" as const,
        roomId: result.roomId,
        newMessages: result.newMessages,
        omitted: result.omitted,
      };
    }
    // 先记进这一轮，再通知界面：界面读到的消息已经带着它所在的那一轮。
    const { message } = result;
    const runRooms = await observe(
      recordReply(ctx.db, agentId, { roomId: message.roomId, messageId: message.id, body: body.body }),
    );
    notifyMessage(ctx, result);
    notifyRun(ctx, runRooms);
    return { outcome: "posted" as const, id: message.id, roomId: message.roomId, seq: message.seq };
  });
}

/**
 * 运行记录只用于观测，记不上时不能改变 `crew reply` 的结果：消息已经写入或已被 HELD 拦下，
 * 这时返回 500 会让 Agent 以为没发出而重发，或者绕过 HELD。
 */
async function observe<T>(recording: Promise<T>): Promise<T | undefined> {
  try {
    return await recording;
  } catch (error) {
    console.error("[server] 写运行记录失败:", error);
    return undefined;
  }
}
