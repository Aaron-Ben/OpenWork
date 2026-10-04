import { api } from "@crew/protocol";
import type { Express } from "express";
import { notifyMessage, type ServerContext } from "../context";
import { agentOf, route } from "../http";
import { postMessage } from "../messages";

/** Agent 经 `crew` 命令调用的接口。凭证决定是哪个 Agent，请求体里不能指定身份。 */
export function agentRoutes(app: Express, ctx: ServerContext): void {
  route(app, api.agent.reply, async ({ body, response }) => {
    const result = await postMessage(ctx.db, body.roomId, { kind: "agent", id: agentOf(response) }, body.body);
    if (result.kind === "held") {
      return { outcome: "held" as const, newMessages: result.newMessages, omitted: result.omitted };
    }
    notifyMessage(ctx, result);
    return { outcome: "posted" as const, id: result.message.id, seq: result.message.seq };
  });
}
