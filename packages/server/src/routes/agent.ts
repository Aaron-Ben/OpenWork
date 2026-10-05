import { api } from "@crew/protocol";
import type { Express } from "express";
import { notifyMessage, notifyRun, type ServerContext } from "../context";
import { agentOf, route } from "../http";
import { postMessage } from "../messages";
import { recordHeld, recordReply } from "../runs";

/** Agent 经 `crew` 命令调用的接口。凭证决定是哪个 Agent，请求体里不能指定身份。 */
export function agentRoutes(app: Express, ctx: ServerContext): void {
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
