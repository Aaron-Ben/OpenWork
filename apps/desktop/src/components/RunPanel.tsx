import type { DesktopAgent as Agent, AgentId, RoomId, RunEvent, RunSummary } from "@crew/protocol";
import { useState } from "react";
import { cn } from "../lib/cn";
import { useRun, useRuns } from "../lib/queries";
import {
  cacheHitRate,
  describeTool,
  formatCost,
  formatDuration,
  formatTokens,
  type RunTone,
  runOutcome,
  timeline,
  totalTokens,
  triggerText,
} from "../lib/runs";
import { formatMessageTime } from "../lib/time";
import { AgentAvatar } from "./Avatar";

const toneClass: Record<RunTone, string> = {
  live: "border-accent bg-accent text-accent-fg",
  ok: "border-accent text-accent",
  idle: "border-dashed border-line-strong text-faint",
  error: "border-danger text-danger",
  muted: "border-line-strong text-muted",
};

/**
 * 右侧的运行记录面板。从私聊打开时列出这个 Agent 的轮次；从群聊打开时列出群里全部成员的轮次，
 * 可以按成员筛选。选中的一轮展开成时间线。
 */
export function RunPanel({
  title,
  scope,
  agents,
  members,
  selectedId,
  onSelect,
  onClose,
}: {
  title: string;
  scope: { roomId: RoomId } | { agentId: AgentId };
  /** 全部 Agent，用来显示每一轮的名字与头像（离开群聊的成员也要找得到）。 */
  agents: Agent[];
  /** 筛选项：群聊的成员；私聊时为空。 */
  members: Agent[];
  selectedId: string | undefined;
  onSelect(runId: string | undefined): void;
  onClose(): void;
}) {
  const runs = useRuns(scope);
  const [member, setMember] = useState<AgentId>();
  // 从消息打开的一轮可能不在这份列表里：Agent 被别的房间唤醒、却在这里发了言，或者它早于列表的上限。
  // 这时单独读出来，放在最前面。
  const missing = runs.data && selectedId && !runs.data.some((run) => run.id === selectedId) ? selectedId : undefined;
  const extra = useRun(missing);
  const all = [...(missing && extra.data ? [extra.data] : []), ...(runs.data ?? [])];
  const list = all.filter((run) => !member || run.agentId === member);
  const byId = new Map(agents.map((agent) => [agent.id, agent]));
  const showMembers = members.length > 1;

  return (
    <aside className="flex w-[400px] flex-none flex-col border-l border-line bg-bg">
      <header className="drag flex h-[52px] flex-none items-center gap-2.5 border-b border-line pr-3.5 pl-[18px]">
        <b className="truncate text-sm">{title}</b>
        <span className="flex-none font-mono text-[11px] text-faint">{runs.data ? `${list.length} 轮` : ""}</span>
        <button
          type="button"
          aria-label="关闭运行记录"
          onClick={onClose}
          className="ml-auto grid size-7 place-items-center rounded-md text-faint hover:bg-hover hover:text-text"
        >
          ✕
        </button>
      </header>

      {showMembers && (
        <div className="flex flex-wrap gap-1 px-3.5 pt-2.5 pb-1.5 font-mono text-[11.5px]">
          <Chip active={!member} onClick={() => setMember(undefined)}>
            全部成员
          </Chip>
          {members.map((agent) => (
            <Chip key={agent.id} active={member === agent.id} onClick={() => setMember(agent.id)}>
              {agent.displayName}
            </Chip>
          ))}
        </div>
      )}

      <div className="min-h-0 flex-1 overflow-y-auto px-2 pt-1 pb-3">
        {runs.error && <p className="px-2 py-4 text-xs text-danger">读取运行记录失败：{runs.error.message}</p>}
        {runs.isSuccess && list.length === 0 && (
          <p className="px-3 py-6 text-[12.5px] leading-relaxed text-faint">
            还没有运行记录。agent 被唤醒跑一轮后会出现在这里。
          </p>
        )}
        {list.map((run) => (
          <RunRow
            key={run.id}
            run={run}
            agent={byId.get(run.agentId)}
            open={run.id === selectedId}
            onToggle={() => onSelect(run.id === selectedId ? undefined : run.id)}
          />
        ))}
      </div>
    </aside>
  );
}

function Chip({ active, onClick, children }: { active: boolean; onClick(): void; children: string }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "rounded-full border px-2.5 py-[2px]",
        active ? "border-text bg-text text-bg" : "border-line text-muted hover:text-text",
      )}
    >
      {children}
    </button>
  );
}

function RunRow({
  run,
  agent,
  open,
  onToggle,
}: {
  run: RunSummary;
  agent: Agent | undefined;
  open: boolean;
  onToggle(): void;
}) {
  const outcome = runOutcome(run);
  const now = new Date();
  const duration = formatDuration(new Date(run.endedAt ?? now).getTime() - new Date(run.startedAt).getTime());
  return (
    <div className={cn("my-0.5 rounded-lg text-[12.5px]", open && "bg-panel shadow-[0_0_0_1px_var(--line-strong)]")}>
      <button type="button" onClick={onToggle} className="block w-full rounded-lg px-2.5 py-2 text-left hover:bg-hover">
        <span className="flex items-center gap-2">
          {agent && <AgentAvatar name={agent.displayName} handle={agent.handle} size={18} />}
          <b className="font-semibold">{agent?.displayName ?? "已删除的 agent"}</b>
          <span className={cn("rounded border px-1.5 font-mono text-[10.5px]", toneClass[outcome.tone])}>
            {outcome.label}
          </span>
          <span className="ml-auto font-mono text-[11px] text-faint">{formatMessageTime(run.startedAt, now)}</span>
        </span>
        <span className="mt-0.5 flex flex-wrap gap-x-2.5 font-mono text-[11px] text-faint">
          <span>{duration}</span>
          <span>{run.steps} 步</span>
          {totalTokens(run.usage) > 0 && <span>{formatTokens(totalTokens(run.usage))} tokens</span>}
          {run.usage.cost > 0 && <span>{formatCost(run.usage.cost)}</span>}
          {run.holds > 0 && <span className="text-warn">HELD {run.holds} 次</span>}
          {run.error && <span className="truncate text-danger">{run.error}</span>}
        </span>
      </button>
      {open && <RunDetailView runId={run.id} />}
    </div>
  );
}

function RunDetailView({ runId }: { runId: string }) {
  const { data: run, error } = useRun(runId);
  if (error) return <p className="px-3 pb-3 text-xs text-danger">读取这一轮失败：{error.message}</p>;
  if (!run) return null;
  const hit = cacheHitRate(run.usage);
  let step = 0;
  return (
    <div className="px-2.5 pb-3">
      <div className="my-2 grid grid-cols-3 gap-1.5">
        <Stat label="输入" value={formatTokens(run.usage.input + run.usage.cacheRead + run.usage.cacheWrite)} />
        <Stat label="输出" value={formatTokens(run.usage.output + run.usage.reasoning)} />
        <Stat label="缓存命中" value={hit === null ? "—" : `${Math.round(hit * 100)}%`} />
      </div>
      <div className="rounded-md bg-hover px-2 py-1.5 font-mono text-[11.5px] text-muted">
        被{triggerText(run.triggers)}唤醒
      </div>
      <details className="mt-1.5 font-mono text-[11.5px] text-muted">
        <summary className="cursor-pointer select-none text-faint">本轮输入</summary>
        <pre className="selectable mt-1 max-h-64 overflow-auto rounded-md bg-hover px-2 py-1.5 text-[11px] leading-relaxed whitespace-pre-wrap">
          {run.prompt}
        </pre>
      </details>
      <ol className="mt-2 ml-1 border-l border-line-strong pl-3 font-mono text-[11.5px] leading-relaxed">
        {timeline(run.events).map((event) => {
          if (event.kind === "step") step += 1;
          return <EventItem key={event.seq} event={event} step={step} />;
        })}
        {run.outcome !== "running" && (
          <Item dot="bg-line-strong">
            <b className="text-text">结束 · {runOutcome(run).label}</b>
            {run.error && <div className="selectable text-danger">{run.error}</div>}
          </Item>
        )}
      </ol>
    </div>
  );
}

function EventItem({ event, step }: { event: RunEvent; step: number }) {
  const time = new Date(event.at).toLocaleTimeString("zh-CN", { hour12: false });
  switch (event.kind) {
    case "step":
      return (
        <Item dot="bg-line-strong" aside={time}>
          <b className="text-text">步骤 {step}</b>
        </Item>
      );
    case "step_end":
      return null;
    case "tool": {
      const tool = describeTool(event);
      return (
        <Item dot="bg-line-strong" aside={event.durationMs === null ? undefined : `${event.durationMs}ms`}>
          <details>
            <summary className={cn("cursor-pointer truncate", event.failed ? "text-danger" : "text-muted")}>
              {tool.mark} {tool.text}
            </summary>
            <pre className="selectable mt-1 max-h-48 overflow-auto rounded-md bg-hover px-2 py-1.5 text-[11px] whitespace-pre-wrap text-muted">
              {event.output || "（没有输出）"}
            </pre>
          </details>
        </Item>
      );
    }
    case "text":
      return (
        <Item dot="bg-line-strong" aside={time}>
          <div className="text-faint">模型的文字（没有人看到）</div>
          <div className="selectable font-sans text-[12.5px] text-muted">{event.text}</div>
        </Item>
      );
    case "reply":
      return (
        <Item dot="bg-accent" aside={time}>
          <b className="text-text">发出回复</b>
          <div className="selectable font-sans text-[12.5px] leading-relaxed text-text">{event.body}</div>
        </Item>
      );
    case "held":
      return (
        <Item dot="bg-warn" aside={time}>
          <b className="text-text">回复被 HELD 拦下</b>
          <div className="font-sans text-[12.5px] text-muted">
            看到 {event.newMessages} 条新消息：“{event.preview}”
          </div>
        </Item>
      );
  }
}

function Item({ dot, aside, children }: { dot: string; aside?: string; children: React.ReactNode }) {
  return (
    <li className="relative py-[3px]">
      <span className={cn("absolute top-[9px] -left-[16px] size-[7px] rounded-full", dot)} />
      <div className="flex gap-2">
        <div className="min-w-0 flex-1">{children}</div>
        {aside && <span className="flex-none text-faint">{aside}</span>}
      </div>
    </li>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-md bg-hover px-2 py-1.5">
      <span className="block font-mono text-[10.5px] text-faint">{label}</span>
      <b className="font-mono text-[13px]">{value}</b>
    </div>
  );
}
