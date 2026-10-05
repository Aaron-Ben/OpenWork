import type { ComputerAgent, EngineEvent, InboxRoom, RunTrigger } from "@crew/protocol";
import type { ServerClient } from "./client";
import type { EngineAdapter } from "./engine/types";
import { type AgentLayout, resumableSession, type SessionKey, saveSession } from "./home";
import { standingInstructions } from "./instructions";
import { turnPrompt } from "./prompt";
import type { Confinement } from "./sandbox";

export interface RunnerOptions {
  agent: ComputerAgent;
  server: RunnerServer;
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
 * 每一轮都记进运行记录：开始时登记，Engine 的每一步随时上报，结束时写结果。
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
        // 读 inbox 或登记这一轮失败（Server 不可达等）：这一轮放弃，下一次唤醒重新读取。
        // 登记之后的失败已经在 handleOnce 里记进了这一轮。
        console.error(`[computer] Agent ${this.agentId} 处理失败:`, error);
      }
    }
  }

  private async handleOnce(): Promise<void> {
    const { agent, server, engine, layout } = this.options;
    const inbox = await server.readInbox(agent.id);
    // 读取期间被停止时不再开始这一轮。
    if (inbox.length === 0 || this.stopped) return;

    const prompt = turnPrompt(inbox, (this.options.now ?? (() => new Date()))(), agent.id);
    const runId = await server.startRun(agent.id, { prompt, triggers: runTriggers(inbox) });
    const reporter = new RunReporter(server, runId);
    const finish = async (result: Parameters<RunnerServer["finishRun"]>[1]) => {
      await reporter.drain();
      await server.finishRun(runId, result);
    };

    try {
      const key: SessionKey = {
        engineId: agent.engineId,
        model: agent.model,
        instructions: standingInstructions(agent),
      };
      const outcome = await engine.runTurn({
        layout,
        confinement: this.options.confinement,
        model: agent.model,
        prompt,
        sessionId: await resumableSession(layout, key),
        env: this.options.env,
        signal: this.controller.signal,
        onEvent: (event) => reporter.push(event),
      });

      if (outcome.ok) {
        await saveSession(layout, key, outcome.sessionId);
        await server.acknowledge(agent.id);
        await finish({ outcome: "succeeded" });
        return;
      }
      if (outcome.error.kind === "cancelled") {
        // 应用正在退出，Server 可能已经不在了；记不上时，下一个 Computer 连上后会把它标为中断。
        await finish({ outcome: "cancelled" }).catch(() => undefined);
        return;
      }
      await finish({ outcome: "failed", error: outcome.error.message });
    } catch (error) {
      // 保存 session、推进已读位置失败，或 Engine 违反约定抛出：记为失败，界面不会一直显示“回复中”。
      console.error(`[computer] Agent ${this.agentId} 处理失败:`, error);
      if (this.stopped) return;
      const reason = `处理失败：${error instanceof Error ? error.message : String(error)}`;
      await finish({ outcome: "failed", error: reason }).catch((reportError: unknown) => {
        console.error(`[computer] Agent ${this.agentId} 记录失败的一轮也失败了:`, reportError);
      });
    }
  }
}

type RunnerServer = Pick<ServerClient, "readInbox" | "acknowledge" | "startRun" | "appendRunEvents" | "finishRun">;

/** 这一轮被每个房间的哪几条消息唤醒。 */
export function runTriggers(inbox: InboxRoom[]): RunTrigger[] {
  return inbox.map((room) => ({
    roomId: room.roomId,
    fromSeq: room.messages[0]?.seq ?? 1,
    toSeq: room.messages.at(-1)?.seq ?? 1,
  }));
}

/**
 * 把 Engine 事件按顺序上报。前一批还在路上时到达的事件攒成下一批，一次只有一个请求，顺序不乱。
 * 上报失败只记日志、丢掉这一批：运行记录是观测用的，不能因为它让这一轮失败。
 */
export class RunReporter {
  private queue: EngineEvent[] = [];
  private sending: Promise<void> | undefined;

  constructor(
    private readonly server: Pick<RunnerServer, "appendRunEvents">,
    private readonly runId: string,
  ) {}

  push(event: EngineEvent): void {
    this.queue.push(event);
    this.sending ??= this.flush().finally(() => {
      this.sending = undefined;
    });
  }

  /** 等已经收到的事件全部上报完。 */
  async drain(): Promise<void> {
    while (this.sending) await this.sending;
  }

  private async flush(): Promise<void> {
    while (this.queue.length > 0) {
      const batch = this.queue.splice(0);
      await this.server.appendRunEvents(this.runId, batch).catch((error: unknown) => {
        console.error(`[computer] 上报运行记录失败（${batch.length} 条，已丢弃）:`, error);
      });
    }
  }
}
