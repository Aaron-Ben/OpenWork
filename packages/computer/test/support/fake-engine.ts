import type { EngineEvent } from "@crew/protocol";
import type { EngineAdapter, TurnOutcome, TurnRequest } from "../../src/engine/types";

/**
 * 按顺序返回预设结果的 Engine；记录每次收到的请求。`hold` 让 Turn 等到测试放行或被中止。
 * `events` 是每一轮开始时依次回调的 Engine 事件。
 */
export class FakeEngine implements EngineAdapter {
  readonly id = "fake";
  readonly requests: TurnRequest[] = [];
  events: EngineEvent[] = [];
  private release: (() => void) | undefined;
  constructor(
    private readonly outcomes: TurnOutcome[],
    private readonly hold = false,
  ) {}
  async probe() {
    return { ready: true as const, executable: "/usr/bin/true" };
  }
  async listModels() {
    return [];
  }
  async runTurn(request: TurnRequest): Promise<TurnOutcome> {
    this.requests.push(request);
    for (const event of this.events) request.onEvent?.(event);
    if (this.hold) {
      const aborted = await new Promise<boolean>((resolve) => {
        this.release = () => resolve(false);
        request.signal.addEventListener("abort", () => resolve(true), { once: true });
      });
      if (aborted) return { ok: false, error: { kind: "cancelled", message: "已停止" } };
    }
    return this.outcomes.shift() ?? { ok: true, sessionId: "ses_default" };
  }
  finishTurn() {
    this.release?.();
  }
}
