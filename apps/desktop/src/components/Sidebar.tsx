import type { DesktopAgent as Agent, DesktopGroup as Group } from "@crew/protocol";
import type { Selection } from "../App";
import { cn } from "../lib/cn";
import type { Connection } from "../lib/events";
import { groupMembers } from "../lib/new-group";
import { statusView } from "../lib/status";
import { StatusTag } from "./StatusTag";
import { Button } from "./ui/button";

/** 左侧栏：群聊、Agent（每个对应一个私聊房间）与连接状态。 */
export function Sidebar({
  agents,
  groups,
  selected,
  onSelect,
  onCreateAgent,
  onCreateGroup,
  connection,
}: {
  agents: Agent[];
  groups: Group[];
  selected: Selection | undefined;
  onSelect(selection: Selection): void;
  onCreateAgent(): void;
  onCreateGroup(): void;
  connection: Connection;
}) {
  return (
    <aside className="flex w-62 flex-none flex-col border-r border-line bg-panel font-mono">
      {/* 顶部留出系统红黄绿按钮的位置，并作为窗口拖动区域。 */}
      <div className="drag px-[18px] pt-[46px]">
        <div className="text-[15px] font-semibold">
          <span className="text-accent">~/</span>crew
        </div>
        <div className="mt-1 text-[11px] text-muted">
          本机 · {agents.length} 个 agent · {groups.length} 个群聊
        </div>
      </div>

      <nav className="min-h-0 flex-1 overflow-y-auto pb-2">
        <SectionHeading title="群聊" onCreate={onCreateGroup} />
        {groups.length === 0 && <p className="px-[18px] py-2 text-xs text-faint">还没有群聊</p>}
        {groups.map((group) => {
          const members = groupMembers(group, agents);
          const working = members.filter((agent) => agent.status.state === "working").length;
          return (
            <Item
              key={group.id}
              active={selected?.kind === "group" && selected.id === group.id}
              onClick={() => onSelect({ kind: "group", id: group.id })}
              title={
                <>
                  <span className="truncate">
                    <span className="text-faint">#</span> {group.name}
                  </span>
                  {working > 0 && (
                    <StatusTag tone="working" className="flex-none text-[11px]">
                      {working} 回复中
                    </StatusTag>
                  )}
                </>
              }
              subtitle={`${members.length} 个 agent`}
            />
          );
        })}

        <SectionHeading title="同事" onCreate={onCreateAgent} />
        {agents.length === 0 && <p className="px-[18px] py-2 text-xs text-faint">还没有 agent</p>}
        {agents.map((agent) => {
          const status = statusView(agent.status);
          return (
            <Item
              key={agent.id}
              active={selected?.kind === "agent" && selected.id === agent.id}
              onClick={() => onSelect({ kind: "agent", id: agent.id })}
              title={
                <>
                  <span className="truncate">{agent.displayName}</span>
                  <StatusTag tone={status.tone} className="flex-none text-[11px]">
                    {status.label}
                  </StatusTag>
                </>
              }
              subtitle={`@${agent.handle} · ${agent.model}`}
            />
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

function SectionHeading({ title, onCreate }: { title: string; onCreate(): void }) {
  return (
    <div className="flex items-center justify-between pt-[22px] pr-3 pb-1.5 pl-[18px] text-[11px] tracking-wide text-faint">
      <span>{title}</span>
      <Button variant="ghost" size="sm" onClick={onCreate}>
        ＋ 新建
      </Button>
    </div>
  );
}

function Item({
  active,
  onClick,
  title,
  subtitle,
}: {
  active: boolean;
  onClick(): void;
  title: React.ReactNode;
  subtitle: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "block w-full border-l-2 border-transparent py-2 pr-[18px] pl-4 text-left hover:bg-hover",
        active && "border-accent bg-hover",
      )}
    >
      <span className="flex items-center justify-between gap-2 text-[13px]">{title}</span>
      <span className="mt-0.5 block truncate text-[11px] text-muted">{subtitle}</span>
    </button>
  );
}
