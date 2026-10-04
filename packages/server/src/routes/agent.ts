import { type AgentId, MessageBody, RoomId } from "@crew/protocol";
import { Hono } from "hono";
import { z } from "zod";
import { notifyMessage, type ServerContext } from "../context";
import { validate } from "../http";
import { appendMessage } from "../messages";

const ReplyBody = z.object({ roomId: RoomId, body: MessageBody });

/** Agent 经 `crew` 命令调用的接口。凭证决定是哪个 Agent，请求体里不能指定身份。 */
export function agentRoutes(ctx: ServerContext) {
  return new Hono<{ Variables: { agentId: AgentId } }>()
    .use(async (c, next) => {
      const token = c.req.header("Authorization")?.match(/^Bearer (.+)$/)?.[1];
      const agentId = token ? ctx.state.agentForToken(token) : undefined;
      if (!agentId) return c.json({ error: "凭证无效" }, 401);
      c.set("agentId", agentId);
      await next();
    })
    .post("/reply", validate("json", ReplyBody), async (c) => {
      const { roomId, body } = c.req.valid("json");
      const result = await appendMessage(ctx.db, roomId, { kind: "agent", id: c.get("agentId") }, body);
      notifyMessage(ctx, result);
      return c.json(result.message, 201);
    });
}
