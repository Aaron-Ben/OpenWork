import { type AgentId, type ComputerAgent, ComputerEvent, type RuntimeSessionId, runEventStream } from "@crew/protocol";
import type { ServerClient } from "./client";
import type { EngineAdapter } from "./engine/types";
import {
  confinementFor,
  prepareAgent,
  prepareRuntime,
  type RuntimeLayout,
  removeRuntime,
  writeAgentToken,
  writeShimWrapper,
} from "./home";
import { AgentRunner } from "./runner";
import { probeSandbox, type SandboxStatus } from "./sandbox";

export interface DaemonOptions {
  client: ServerClient;
  engine: EngineAdapter;
  runtimeSessionId: RuntimeSessionId;
  crewRoot: string;
  /** 运行 shim 的可执行文件。生产中是 Electron 自身（Computer 也由它运行）。 */
  nodeExecutable: string;
  shimEntry: string;
  /** 测试替换沙箱自检。 */
  probe?: () => Promise<SandboxStatus>;
}

interface RunnerEntry {
  runner: AgentRunner;
  /** 影响 Runner 的配置。变化时重建 Runner。 */
  fingerprint: string;
}

function fingerprint(agent: ComputerAgent): string {
  return JSON.stringify([agent.displayName, agent.persona, agent.engineId, agent.model]);
}

/**
 * Computer 的主流程：为每个 Agent 准备目录与凭证并创建 Runner，订阅 Server 的 SSE 把唤醒交给对应的 Runner。
 *
 * 沙箱自检或 Engine 检查不通过时不启动任何 Runner，只把原因上报为每个 Agent 的错误状态，界面据此提示。
 */
export class ComputerDaemon {
  private readonly runners = new Map<AgentId, RunnerEntry>();
  private readonly controller = new AbortController();
  private runtime: RuntimeLayout | undefined;
  private shimWrapper = "";
  private blocked: string | undefined;
  /** 串行执行 reconcile，SSE 重连与“列表变了”的事件可能同时触发它。 */
  private reconciling: Promise<void> = Promise.resolve();
  private events: Promise<void> | undefined;

  constructor(private readonly options: DaemonOptions) {}

  async start(): Promise<void> {
    const { client, engine } = this.options;
    const sandbox = await (this.options.probe ?? probeSandbox)();
    const readiness = await engine.probe();
    if (!sandbox.available) this.blocked = `沙箱不可用：${sandbox.reason}`;
    else if (!readiness.ready) this.blocked = readiness.reason;

    this.runtime = await prepareRuntime(this.options.crewRoot, this.options.runtimeSessionId);
    this.shimWrapper = await writeShimWrapper(this.runtime, this.options.nodeExecutable, this.options.shimEntry);

    if (!this.blocked) {
      void engine
        .listModels()
        .then((models) => client.reportModels(models))
        .catch((error: unknown) => console.error("[computer] 上报模型列表失败:", error));
    }

    this.events = runEventStream({
      ...client.eventStream(),
      schema: ComputerEvent,
      signal: this.controller.signal,
      // 每次连上都重新对齐 Agent 并唤醒全部 Runner，补上断线期间丢失的事件。
      onOpen: () => {
        void this.reconcile().then(() => {
          for (const { runner } of this.runners.values()) runner.wake();
        });
      },
      onEvent: (event) => {
        switch (event.type) {
          case "agent.wake":
            this.runners.get(event.agentId)?.runner.wake();
            return;
          case "agents":
            void this.reconcile();
            return;
        }
      },
      onError: (error) => {
        if (!this.controller.signal.aborted) console.error("[computer] SSE:", error);
      },
    });
  }

  /** 让 Runner 与 Server 上的 Agent 列表一致：新的创建，配置变了的重建，删除的停止。 */
  reconcile(): Promise<void> {
    this.reconciling = this.reconciling.then(
      () => this.reconcileOnce(),
      () => this.reconcileOnce(),
    );
    return this.reconciling.catch((error: unknown) => console.error("[computer] 同步 Agent 列表失败:", error));
  }

  private async reconcileOnce(): Promise<void> {
    if (this.controller.signal.aborted) return;
    const { client } = this.options;
    const agents = await client.listAgents();

    if (this.blocked) {
      const reason = this.blocked;
      await Promise.all(agents.map((agent) => client.reportStatus(agent.id, { state: "error", reason })));
      return;
    }

    const wanted = new Set(agents.map((agent) => agent.id));
    for (const [agentId, entry] of this.runners) {
      if (!wanted.has(agentId)) {
        this.runners.delete(agentId);
        await entry.runner.stop();
      }
    }
    for (const agent of agents) {
      const current = this.runners.get(agent.id);
      if (current?.fingerprint === fingerprint(agent)) continue;
      if (current) await current.runner.stop();
      // 停止开始后不再创建 Runner。
      if (this.controller.signal.aborted) return;
      let runner: AgentRunner;
      try {
        runner = await this.createRunner(agent);
      } catch (error) {
        // 一个 Agent 准备失败（例如它的目录被换成了符号链接）不影响其他 Agent；原因上报给界面。
        const reason = `准备 Agent 失败：${error instanceof Error ? error.message : String(error)}`;
        console.error(`[computer] ${reason}`);
        if (current) this.runners.delete(agent.id);
        await client.reportStatus(agent.id, { state: "error", reason });
        continue;
      }
      this.runners.set(agent.id, { runner, fingerprint: fingerprint(agent) });
      runner.wake();
    }
  }

  private async createRunner(agent: ComputerAgent): Promise<AgentRunner> {
    const runtime = this.runtime;
    if (!runtime) throw new Error("Computer 还没有启动");
    const { client, engine } = this.options;
    const layout = await prepareAgent(runtime, agent);
    await writeAgentToken(layout, await client.issueAgentToken(agent.id));
    const readiness = await engine.probe();
    const executables = readiness.ready ? [readiness.executable] : [];
    return new AgentRunner({
      agent,
      server: client,
      engine,
      layout,
      confinement: await confinementFor(runtime, layout, executables),
      env: {
        PATH: `${runtime.binDir}:${process.env.PATH ?? "/usr/bin:/bin"}`,
        CREW_SERVER_URL: client.baseUrl,
        CREW_TOKEN_FILE: layout.tokenFile,
      },
    });
  }

  /** `bin/crew` 的路径。 */
  get shimPath(): string {
    return this.shimWrapper;
  }

  /** 停止 SSE 与全部 Runner（中止正在运行的 Turn），删除本次运行的目录。 */
  async stop(): Promise<void> {
    this.controller.abort();
    // 先同时中止全部 Runner，再等 SSE 与正在进行的同步结束。反过来时，一次同步里重建 Runner 的等待
    // 会推迟其他 Runner 的中止，总时长可能超过主进程给的宽限，Computer 被强制结束，Engine 成为孤儿。
    const stopping = [...this.runners.values()].map(({ runner }) => runner.stop());
    await Promise.all([this.events, this.reconciling.catch(() => undefined), ...stopping]);
    // 同步在停止前创建的 Runner 也要停掉。
    await Promise.all([...this.runners.values()].map(({ runner }) => runner.stop()));
    this.runners.clear();
    if (this.runtime) await removeRuntime(this.runtime);
  }
}
