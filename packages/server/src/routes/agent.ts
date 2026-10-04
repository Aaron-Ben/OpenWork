import { api } from "@crew/protocol";
import type { Express } from "express";
import { notifyMessage, type ServerContext } from "../context";
import { agentOf, route } from "../http";
import { appendMessage } from "../messages";

/** Agent 经 `crew` 命令调用的接口。凭证决定是哪个 Agent，请求体里不能指定身份。 */
export function agentRoutes(app: Express, ctx: ServerContext): void {
  route(app, api.agent.reply, async ({ body, response }) => {
    const result = await appendMessage(ctx.db, body.roomId, { kind: "agent", id: agentOf(response) }, body.body);
    notifyMessage(ctx, result);
    return result.message;
  });
}
