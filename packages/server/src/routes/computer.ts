import { api, EVENT_STREAMS } from "@crew/protocol";
import type { Express } from "express";
import { assertAgentExists, listAgents } from "../agents";
import type { ServerContext } from "../context";
import { eventStream, route } from "../http";
import { acknowledge, readInbox } from "../messages";

/** Computer 调用的接口。凭证在 app.ts 中按路径前缀统一处理。 */
export function computerRoutes(app: Express, ctx: ServerContext): void {
  // Computer 启动时调用，确认地址与凭证可用。
  route(app, api.computer.connect, () => undefined);

  route(app, api.computer.listAgents, () => listAgents(ctx.db));

  route(app, api.computer.readInbox, ({ params }) => readInbox(ctx.db, params.agentId));

  route(app, api.computer.acknowledge, async ({ params, body }) => {
    for (const { roomId, seq } of body.acks) {
      await acknowledge(ctx.db, params.agentId, roomId, seq);
    }
  });

  route(app, api.computer.issueAgentToken, async ({ params }) => {
    await assertAgentExists(ctx.db, params.agentId);
    return { token: ctx.state.issueAgentToken(params.agentId) };
  });

  route(app, api.computer.reportStatus, async ({ params, body }) => {
    await assertAgentExists(ctx.db, params.agentId);
    ctx.state.setStatus(params.agentId, body);
    ctx.events.desktop.publish({ type: "agents" });
  });

  route(app, api.computer.reportModels, ({ body }) => {
    ctx.state.setModels(body.models);
    ctx.events.desktop.publish({ type: "models" });
  });

  app.get(EVENT_STREAMS.computer, (request, response) => eventStream(request, response, ctx.events.computer));
}
