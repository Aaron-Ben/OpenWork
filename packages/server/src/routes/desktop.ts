import { api, EVENT_STREAMS } from "@crew/protocol";
import type { Express } from "express";
import { type AgentSummary, createAgent, listAgents } from "../agents";
import { notifyMessage, type ServerContext } from "../context";
import { eventStream, route } from "../http";
import { appendMessage, listMessages } from "../messages";

/** 界面调用的接口。凭证与 CORS 在 app.ts 中按路径前缀统一处理。 */
export function desktopRoutes(app: Express, ctx: ServerContext): void {
  const view = (agent: AgentSummary) => ({
    ...agent,
    createdAt: agent.createdAt.toISOString(),
    status: ctx.state.statusOf(agent.id),
  });

  route(app, api.desktop.listAgents, async () => (await listAgents(ctx.db)).map(view));

  route(app, api.desktop.createAgent, async ({ body }) => {
    const agent = await createAgent(ctx.db, ctx.localUserId, body);
    ctx.events.desktop.publish({ type: "agents" });
    ctx.events.computer.publish({ type: "agents" });
    return view(agent);
  });

  route(app, api.desktop.listMessages, ({ params }) => listMessages(ctx.db, params.roomId));

  route(app, api.desktop.sendMessage, async ({ params, body }) => {
    const result = await appendMessage(ctx.db, params.roomId, { kind: "user", id: ctx.localUserId }, body.body);
    notifyMessage(ctx, result);
    return result.message;
  });

  route(app, api.desktop.listModels, () => ctx.state.listModels());

  app.get(EVENT_STREAMS.desktop, (request, response) => eventStream(request, response, ctx.events.desktop));
}
