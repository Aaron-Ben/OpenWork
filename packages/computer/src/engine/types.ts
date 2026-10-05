import type { EngineEvent } from "@crew/protocol";
import type { AgentLayout } from "../home";
import type { Confinement } from "../sandbox";

/** Engine 失败的原因。Runner 按 `kind` 决定怎样报告，界面显示 `message`。 */
export type EngineError =
  /** 没有可用的登录凭证。 */
  | { kind: "unauthenticated"; message: string }
  /** 模型不可用：没有登录对应的服务，或模型名不对。 */
  | { kind: "model-unavailable"; message: string }
  | { kind: "rate-limited"; message: string }
  /** 旧 session 不存在。适配器会开新 session 重跑一次，重跑仍失败时才报告。 */
  | { kind: "session-invalid"; message: string }
  /** `sandbox-exec` 在启动 Engine 之前就失败了。 */
  | { kind: "sandbox"; message: string }
  /** Engine 在输出中报告了错误。 */
  | { kind: "reported"; message: string }
  /** Engine 进程异常退出，或输出不符合预期。 */
  | { kind: "process"; message: string }
  | { kind: "output-limit"; message: string }
  | { kind: "cancelled"; message: string };

export type TurnOutcome = { ok: true; sessionId: string } | { ok: false; error: EngineError };

export interface TurnRequest {
  layout: AgentLayout;
  confinement: Confinement;
  model: string;
  prompt: string;
  /** 继续这个 session；没有时开新 session。 */
  sessionId: string | undefined;
  /** 传给 Engine 进程的额外环境变量，例如 shim 需要的 Server 地址。 */
  env: Record<string, string>;
  /** 中止时结束整个 Engine 进程组。 */
  signal: AbortSignal;
  /** Engine 的每一步：开始一步、调用工具、输出文字、一步结束及其用量。按发生顺序回调。 */
  onEvent?: (event: EngineEvent) => void;
}

export type EngineReadiness = { ready: true; executable: string } | { ready: false; reason: string };

export interface EngineAdapter {
  readonly id: string;
  /** 确认 Engine 可执行文件存在，不启动它。 */
  probe(): Promise<EngineReadiness>;
  /** 本机可用的模型 id。 */
  listModels(): Promise<string[]>;
  /** 运行一个 Turn。不抛出：所有失败都以 `{ ok: false }` 返回。 */
  runTurn(request: TurnRequest): Promise<TurnOutcome>;
}
