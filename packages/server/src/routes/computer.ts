import { api, EVENT_STREAMS } from "@crew/protocol";
import type { Express } from "express";
import { assertAgentExists, listAgents } from "../agents";
import { notifyRun, type ServerContext } from "../context";
import { eventStream, route } from "../http";
import { acknowledge, readInbox } from "../messages";
import { appendEngineEvents, finishRun, interruptRunning, startRun } from "../runs";

/** Computer 调用的接口。凭证在 app.ts 中按路径前缀统一处理。 */
export function computerRoutes(app: Express, ctx: ServerContext): void {
  // Computer 启动时调用，确认地址与凭证可用。上一个 Computer 没结束的轮次不会再有结果，标为中断。
  route(app, api.computer.connect, async () => {
    const interrupted = await interruptRunning(ctx.db);
    for (const runId of interrupted) notifyRun(ctx, { runId, roomIds: [] });
    if (interrupted.length > 0) ctx.events.desktop.publish({ type: "agents" });
  });

  route(app, api.computer.listAgents, () => listAgents(ctx.db));

  route(app, api.computer.readInbox, ({ params }) => readInbox(ctx.db, params.agentId));

  route(app, api.computer.acknowledge, ({ params }) => acknowledge(ctx.db, params.agentId));

  route(app, api.computer.issueAgentToken, async ({ params }) => {
    await assertAgentExists(ctx.db, params.agentId);
    return { token: ctx.state.issueAgentToken(params.agentId) };
  });

  route(app, api.computer.reportProblem, async ({ params, body }) => {
    await assertAgentExists(ctx.db, params.agentId);
    ctx.state.setProblem(params.agentId, body.problem);
    ctx.events.desktop.publish({ type: "agents" });
  });

  route(app, api.computer.startRun, async ({ params, body }) => {
    await assertAgentExists(ctx.db, params.agentId);
    const run = await startRun(ctx.db, params.agentId, body);
    notifyRun(ctx, { runId: run.id, roomIds: run.roomIds }, true);
    return { id: run.id };
  });

  route(app, api.computer.appendRunEvents, async ({ params, body }) => {
    const roomIds = await appendEngineEvents(ctx.db, params.runId, body.events);
    notifyRun(ctx, { runId: params.runId, roomIds });
  });

  route(app, api.computer.finishRun, async ({ params, body }) => {
    const { roomIds } = await finishRun(ctx.db, params.runId, body);
    notifyRun(ctx, { runId: params.runId, roomIds }, true);
  });

  route(app, api.computer.reportModels, ({ body }) => {
    ctx.state.setModels(body.models);
    ctx.events.desktop.publish({ type: "models" });
  });

  app.get(EVENT_STREAMS.computer, (request, response) => eventStream(request, response, ctx.events.computer));
}
