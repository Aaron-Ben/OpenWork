import { MessageBody, RoomId } from "@crew/protocol";
import { Hono } from "hono";
import { z } from "zod";
import { createAgent, listAgents } from "../agents";
import { notifyMessage, type ServerContext } from "../context";
import { eventStream, requireToken, validate } from "../http";
import { appendMessage, listMessages } from "../messages";

const NewAgentBody = z.object({
  displayName: z.string().trim().min(1, "名字不能为空").max(40, "名字最多 40 字符"),
  persona: z.string().trim().min(1, "人设不能为空").max(4_000, "人设最多 4,000 字符"),
  model: z.string().min(1, "请选择模型"),
});

const RoomParam = z.object({ roomId: RoomId });

/** 界面调用的接口，只接受 Desktop 凭证。 */
export function desktopRoutes(ctx: ServerContext, desktopToken: string) {
  return new Hono()
    .use(requireToken(desktopToken))
    .get("/agents", async (c) => {
      const agents = await listAgents(ctx.db);
      return c.json(agents.map((agent) => ({ ...agent, status: ctx.state.statusOf(agent.id) })));
    })
    .post("/agents", validate("json", NewAgentBody), async (c) => {
      const agent = await createAgent(ctx.db, ctx.localUserId, c.req.valid("json"));
      ctx.events.desktop.publish({ type: "agents" });
      ctx.events.computer.publish({ type: "agents" });
      return c.json({ ...agent, status: ctx.state.statusOf(agent.id) }, 201);
    })
    .get("/rooms/:roomId/messages", validate("param", RoomParam), async (c) => {
      return c.json(await listMessages(ctx.db, c.req.valid("param").roomId));
    })
    .post(
      "/rooms/:roomId/messages",
      validate("param", RoomParam),
      validate("json", z.object({ body: MessageBody })),
      async (c) => {
        const result = await appendMessage(
          ctx.db,
          c.req.valid("param").roomId,
          { kind: "user", id: ctx.localUserId },
          c.req.valid("json").body,
        );
        notifyMessage(ctx, result);
        return c.json(result.message, 201);
      },
    )
    .get("/models", (c) => c.json(ctx.state.listModels()))
    .get("/events", (c) => eventStream(c, ctx.events.desktop));
}
