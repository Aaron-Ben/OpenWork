import type { ComputerAgent, InboxRoom } from "@crew/protocol";
import type { ServerClient } from "./client";
import type { EngineAdapter } from "./engine/types";
import { type AgentLayout, resumableSession, type SessionKey, saveSession } from "./home";
import { standingInstructions } from "./instructions";
import { turnPrompt } from "./prompt";
import type { Confinement } from "./sandbox";

export interface RunnerOptions {
  agent: ComputerAgent;
  server: Pick<ServerClient, "readInbox" | "acknowledge" | "reportStatus">;
  engine: EngineAdapter;
  layout: AgentLayout;
  confinement: Confinement;
  /** 传给 Engine 进程的环境变量，例如 shim 需要的 Server 地址与凭证文件。 */
  env: Record<string, string>;
  now?: () => Date;
}

/**
 * 一个 Agent 的执行者。内部只有一个串行的处理循环，所以同一个 Agent 同时最多运行一个 Turn。
 *
 * `wake()` 只做标记：空闲时立即开始处理；正在运行时，Turn 结束后再处理一轮，
 * 期间的多次唤醒合并成这一轮。每轮重新读取 inbox，不在内存中积累消息。
 *
 * Turn 成功后推进已读位置；失败时不推进，下一次唤醒重新处理同样的消息。
 */
export class AgentRunner {
  private pending = false;
  private stopped = false;
  private loop: Promise<void> | undefined;
  private readonly controller = new AbortController();

  constructor(private readonly options: RunnerOptions) {}

  get agentId() {
    return this.options.agent.id;
  }

  wake(): void {
    if (this.stopped) return;
    this.pending = true;
    this.loop ??= this.run().finally(() => {
      this.loop = undefined;
    });
  }

  /** 中止正在运行的 Turn，等处理循环结束。之后的唤醒被忽略。 */
  async stop(): Promise<void> {
    this.stopped = true;
    this.controller.abort();
    await this.loop;
  }

  /** 等当前的处理循环结束。测试用它确认唤醒已经处理完。 */
  async idle(): Promise<void> {
    await this.loop;
  }

  private async run(): Promise<void> {
    while (this.pending && !this.stopped) {
      this.pending = false;
      try {
        await this.handleOnce();
      } catch (error) {
        // 读 inbox、上报状态或推进已读位置失败：这一轮放弃，下一次唤醒重新读取。
        console.error(`[computer] Agent ${this.agentId} 处理失败:`, error);
      }
    }
  }

  private async handleOnce(): Promise<void> {
    const { agent, server, engine, layout } = this.options;
    const inbox = await server.readInbox(agent.id);
    if (inbox.length === 0) return;

    await server.reportStatus(agent.id, { state: "working" });
    const key: SessionKey = { engineId: agent.engineId, model: agent.model, instructions: standingInstructions(agent) };
    const outcome = await engine.runTurn({
      layout,
      confinement: this.options.confinement,
      model: agent.model,
      prompt: turnPrompt(inbox, (this.options.now ?? (() => new Date()))()),
      sessionId: await resumableSession(layout, key),
      env: this.options.env,
      signal: this.controller.signal,
    });

    if (outcome.ok) {
      await saveSession(layout, key, outcome.sessionId);
      await server.acknowledge(agent.id, lastSeqs(inbox));
      await server.reportStatus(agent.id, { state: "idle" });
      return;
    }
    // 停止时不再上报：应用正在退出，Server 可能已经不在了。
    if (outcome.error.kind === "cancelled") return;
    await server.reportStatus(agent.id, { state: "error", reason: outcome.error.message });
  }
}

/** 每个房间本批最后一条消息的序号。 */
function lastSeqs(inbox: InboxRoom[]) {
  return inbox.map((room) => ({ roomId: room.roomId, seq: room.messages.at(-1)?.seq ?? 0 }));
}
