import type { RunDetail, RunEvent, RunSummary, RunTrigger, Usage } from "@crew/protocol";

// 运行记录在界面上的写法：结果标签、时长、token、费用，以及时间线里每一步怎样显示。

export type RunTone = "live" | "ok" | "idle" | "error" | "muted";

/** 一轮的结果标签。成功但一条消息也没发出的一轮是“白跑”。 */
export function runOutcome(run: Pick<RunSummary, "outcome" | "replies">): { label: string; tone: RunTone } {
  switch (run.outcome) {
    case "running":
      return { label: "进行中", tone: "live" };
    case "succeeded":
      return run.replies > 0 ? { label: `发出 ${run.replies} 条`, tone: "ok" } : { label: "白跑", tone: "idle" };
    case "failed":
      return { label: "失败", tone: "error" };
    case "cancelled":
      return { label: "已停止", tone: "muted" };
    case "interrupted":
      return { label: "中断", tone: "muted" };
  }
}

/** 时长：不到一分钟写秒，否则写分秒。 */
export function formatDuration(ms: number): string {
  const seconds = Math.max(0, Math.round(ms / 1000));
  if (seconds < 60) return `${seconds}s`;
  return `${Math.floor(seconds / 60)}m${String(seconds % 60).padStart(2, "0")}s`;
}

/** 工具耗时：不到一秒写毫秒。 */
export function formatToolDuration(ms: number | null): string {
  if (ms === null) return "";
  return ms < 1000 ? `${ms}ms` : `${(ms / 1000).toFixed(1)}s`;
}

/** token 数：一千以上写成 18.2k。 */
export function formatTokens(count: number): string {
  return count < 1000 ? String(count) : `${(count / 1000).toFixed(1)}k`;
}

/** 费用（美元）。没有费用信息时 Engine 报 0，显示为 —。 */
export function formatCost(usd: number): string {
  if (usd === 0) return "—";
  return usd < 0.01 ? `$${usd.toFixed(4)}` : `$${usd.toFixed(2)}`;
}

/** 一轮的 token 总数：未缓存的输入、缓存读、输出与推理。 */
export function totalTokens(usage: Usage): number {
  return usage.input + usage.cacheRead + usage.cacheWrite + usage.output + usage.reasoning;
}

/** 输入里命中缓存的比例；没有输入时为 null。 */
export function cacheHitRate(usage: Usage): number | null {
  const input = usage.input + usage.cacheRead + usage.cacheWrite;
  return input === 0 ? null : usage.cacheRead / input;
}

type ToolEvent = Extract<RunEvent, { kind: "tool" }>;

/** 工具调用的一行写法：命令前加 $，读文件写“读取”，其余写工具名与标题。 */
export function describeTool(event: Pick<ToolEvent, "tool" | "title" | "input">): { mark: string; text: string } {
  const input = parseInput(event.input);
  switch (event.tool) {
    case "bash":
      return { mark: "$", text: typeof input.command === "string" ? input.command : event.title };
    case "read":
      return {
        mark: "↗",
        text: `读取 ${shortPath(typeof input.filePath === "string" ? input.filePath : event.title)}`,
      };
    case "write":
    case "edit":
      return {
        mark: "✎",
        text: `写入 ${shortPath(typeof input.filePath === "string" ? input.filePath : event.title)}`,
      };
    default:
      return { mark: "·", text: event.title ? `${event.tool} ${event.title}` : event.tool };
  }
}

function parseInput(input: string): Record<string, unknown> {
  try {
    const value: unknown = JSON.parse(input);
    return typeof value === "object" && value !== null ? (value as Record<string, unknown>) : {};
  } catch {
    // 截短过的输入不是完整的 JSON：按没有参数处理，界面退回显示标题。
    return {};
  }
}

/** 路径只留最后两段：Agent 目录在一个很长的临时路径下。 */
function shortPath(path: string): string {
  return path.split("/").filter(Boolean).slice(-2).join("/");
}

/** `crew reply` 这次工具调用与随后的“发出回复”是同一件事，时间线里只显示后者。 */
export function isReplyCommand(event: RunEvent): boolean {
  if (event.kind !== "tool" || event.tool !== "bash") return false;
  const command = parseInput(event.input).command;
  return typeof command === "string" && /^\s*crew\s+reply\b/.test(command);
}

/** 时间线里显示的步骤：去掉与“发出回复”重复的 `crew reply` 工具调用。 */
export function timeline(events: readonly RunEvent[]): RunEvent[] {
  return events.filter((event) => !isReplyCommand(event));
}

export interface LiveView {
  /** 最近几步工具调用，从旧到新。 */
  recent: Array<{ seq: number; mark: string; text: string; duration: string }>;
  /** 模型正在想下一步：最近一步开始了，还没有结束。 */
  thinking: boolean;
  /** 已经开始的步数。 */
  steps: number;
}

/** 聊天里实时活动框的内容：最近 `count` 次工具调用，以及模型是不是正在思考。 */
export function liveView(run: Pick<RunDetail, "events">, count = 3): LiveView {
  const events = timeline(run.events);
  const recent = events
    .filter((event): event is Extract<RunEvent, { kind: "tool" }> => event.kind === "tool")
    .slice(-count)
    .map((event) => ({ seq: event.seq, ...describeTool(event), duration: formatToolDuration(event.durationMs) }));
  const lastStep = events.findLast((event) => event.kind === "step" || event.kind === "step_end");
  return {
    recent,
    thinking: lastStep?.kind === "step",
    steps: events.filter((event) => event.kind === "step").length,
  };
}

/** 一轮被哪些消息唤醒：“第 3 条消息”“第 3–5 条消息”；涉及几个房间时写“3 个房间的消息”。 */
export function triggerText(triggers: readonly RunTrigger[]): string {
  const [only] = triggers;
  if (triggers.length !== 1 || !only) return `${triggers.length} 个房间的消息`;
  const range = only.toSeq > only.fromSeq ? `${only.fromSeq}–${only.toSeq}` : String(only.fromSeq);
  return `第 ${range} 条消息`;
}
