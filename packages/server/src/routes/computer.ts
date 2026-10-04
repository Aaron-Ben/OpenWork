import { AgentId, AgentStatus, RoomId } from "@crew/protocol";
import { Hono } from "hono";
import { z } from "zod";
import { assertAgentExists, listAgents } from "../agents";
import type { ServerContext } from "../context";
import { eventStream, requireToken, validate } from "../http";
import { acknowledge, readInbox } from "../messages";

const AgentParam = z.object({ agentId: AgentId });

const AckBody = z.object({
  acks: z.array(z.object({ roomId: RoomId, seq: z.number().int().nonnegative() })).min(1),
});

/** Computer 调用的接口，只接受 Computer 凭证。 */
export function computerRoutes(ctx: ServerContext, computerToken: string) {
  return new Hono()
    .use(requireToken(computerToken))
    .post("/connect", (c) => {
      ctx.state.computerConnected = true;
      return c.body(null, 204);
    })
    .get("/agents", async (c) => c.json(await listAgents(ctx.db)))
    .get("/events", (c) => eventStream(c, ctx.events.computer))
    .get("/agents/:agentId/inbox", validate("param", AgentParam), async (c) => {
      return c.json(await readInbox(ctx.db, c.req.valid("param").agentId));
    })
    .post("/agents/:agentId/inbox/ack", validate("param", AgentParam), validate("json", AckBody), async (c) => {
      const { agentId } = c.req.valid("param");
      for (const { roomId, seq } of c.req.valid("json").acks) {
        await acknowledge(ctx.db, agentId, roomId, seq);
      }
      return c.body(null, 204);
    })
    .post("/agents/:agentId/token", validate("param", AgentParam), async (c) => {
      const { agentId } = c.req.valid("param");
      await assertAgentExists(ctx.db, agentId);
      return c.json({ token: ctx.state.issueAgentToken(agentId) });
    })
    .post("/agents/:agentId/status", validate("param", AgentParam), validate("json", AgentStatus), async (c) => {
      const { agentId } = c.req.valid("param");
      await assertAgentExists(ctx.db, agentId);
      ctx.state.setStatus(agentId, c.req.valid("json"));
      ctx.events.desktop.publish({ type: "agents" });
      return c.body(null, 204);
    })
    .post("/models", validate("json", z.object({ models: z.array(z.string().min(1)) })), (c) => {
      ctx.state.setModels(c.req.valid("json").models);
      return c.body(null, 204);
    });
}
