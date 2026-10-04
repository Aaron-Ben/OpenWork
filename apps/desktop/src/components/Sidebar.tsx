import type { AgentId } from "@crew/protocol";
import { cn } from "../lib/cn";
import type { Connection } from "../lib/events";
import type { Agent } from "../lib/server";
import { statusView } from "../lib/status";
import { StatusTag } from "./StatusTag";
import { Button } from "./ui/button";

/** 左侧栏：Agent 列表与连接状态。每个 Agent 对应一个私聊房间。 */
export function Sidebar({
  agents,
  selectedId,
  onSelect,
  onCreate,
  connection,
}: {
  agents: Agent[];
  selectedId: AgentId | undefined;
  onSelect(id: AgentId): void;
  onCreate(): void;
  connection: Connection;
}) {
  return (
    <aside className="flex w-62 flex-none flex-col border-r border-line bg-panel font-mono">
      {/* 顶部留出系统红黄绿按钮的位置，并作为窗口拖动区域。 */}
      <div className="drag px-[18px] pt-[46px]">
        <div className="text-[15px] font-semibold">
          <span className="text-accent">~/</span>crew
        </div>
        <div className="mt-1 text-[11px] text-muted">本机 · {agents.length} 个 agent</div>
      </div>

      <div className="flex items-center justify-between pt-[22px] pr-3 pb-1.5 pl-[18px] text-[11px] tracking-wide text-faint">
        <span>同事</span>
        <Button variant="ghost" size="sm" onClick={onCreate}>
          ＋ 新建
        </Button>
      </div>

      <nav className="min-h-0 flex-1 overflow-y-auto">
        {agents.length === 0 && <p className="px-[18px] py-2 text-xs text-faint">还没有 agent</p>}
        {agents.map((agent) => {
          const status = statusView(agent.status);
          return (
            <button
              key={agent.id}
              type="button"
              onClick={() => onSelect(agent.id)}
              className={cn(
                "block w-full border-l-2 border-transparent py-2 pr-[18px] pl-4 text-left hover:bg-hover",
                agent.id === selectedId && "border-accent bg-hover",
              )}
            >
              <span className="flex items-center justify-between gap-2 text-[13px]">
                <span className="truncate">{agent.displayName}</span>
                <StatusTag tone={status.tone} className="flex-none text-[11px]">
                  {status.label}
                </StatusTag>
              </span>
              <span className="mt-0.5 block truncate text-[11px] text-muted">{agent.model}</span>
            </button>
          );
        })}
      </nav>

      <div className="border-t border-line px-[18px] py-3 text-[11px]">
        {connection === "reconnecting" ? (
          <StatusTag tone="reconnecting" className="text-[11px]">
            正在重新连接…
          </StatusTag>
        ) : connection === "connected" ? (
          <StatusTag tone="working" className="text-[11px]">
            已连接
          </StatusTag>
        ) : (
          <StatusTag tone="idle" className="text-[11px]">
            连接中…
          </StatusTag>
        )}
      </div>
    </aside>
  );
}
