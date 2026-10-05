import type { DesktopAgent as Agent, Conversation, DesktopGroup as Group, RoomId } from "@crew/protocol";
import { useState } from "react";
import { cn } from "../lib/cn";
import { conversationPreview, totalUnread, unreadLabel } from "../lib/conversations";
import type { Connection } from "../lib/events";
import { groupMembers } from "../lib/new-group";
import { statusView } from "../lib/status";
import { formatListTime } from "../lib/time";
import { AgentAvatar, GroupAvatar } from "./Avatar";
import { SidebarToggle } from "./SidePanel";
import { StatusTag } from "./StatusTag";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "./ui/dropdown-menu";

type Tab = "messages" | "groups" | "contacts";

const TABS: Array<{ id: Tab; label: string }> = [
  { id: "messages", label: "消息" },
  { id: "groups", label: "群聊" },
  { id: "contacts", label: "联系人" },
];

/**
 * 左侧栏：顶部用分段切换三个列表，点击任何一项都在右侧打开对应的聊天。
 * 消息是全部会话，按最后活动排列；群聊与联系人是两份目录。
 */
export function Sidebar({
  agents,
  groups,
  conversations,
  selectedRoomId,
  onSelect,
  onCreateAgent,
  onCreateGroup,
  connection,
  onHide,
}: {
  agents: Agent[];
  groups: Group[];
  conversations: Conversation[];
  selectedRoomId: RoomId | undefined;
  onSelect(roomId: RoomId): void;
  onCreateAgent(): void;
  onCreateGroup(): void;
  connection: Connection;
  /** 没有打开的聊天时为空：那时侧栏不能收起。 */
  onHide?(): void;
}) {
  const [tab, setTab] = useState<Tab>("messages");
  const unread = totalUnread(conversations);
  const now = new Date();
  const membersOf = (agentIds: readonly string[]) =>
    agents
      .filter((agent) => agentIds.includes(agent.id))
      .map((agent) => ({ name: agent.displayName, handle: agent.handle }));

  return (
    <aside className="flex w-72 flex-none flex-col bg-panel">
      {/* 第一行左边是系统的红黄绿按钮，右边是“隐藏侧栏”；第二行是“新建”与连接状态。两行都是窗口拖动区域。 */}
      <div className="drag flex h-[52px] flex-none items-center justify-end px-2.5">
        {onHide && <SidebarToggle hidden={false} onClick={onHide} />}
      </div>
      <div className="drag flex items-center justify-between px-3.5 pb-2.5">
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button
              type="button"
              className="inline-flex h-[30px] items-center gap-1.5 rounded-[9px] border border-line bg-raised pr-2.5 pl-2 text-[13px] font-medium shadow-card outline-none hover:border-line-strong focus-visible:ring-3 focus-visible:ring-accent-soft data-[state=open]:border-line-strong"
            >
              <span className="grid size-[18px] place-items-center rounded-md bg-accent text-sm leading-none text-accent-fg">
                +
              </span>
              新建
              <span className="text-faint">⌄</span>
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start">
            <DropdownMenuItem onSelect={onCreateAgent}>新建 agent</DropdownMenuItem>
            <DropdownMenuItem onSelect={onCreateGroup} disabled={agents.length === 0}>
              新建群聊
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
        <ConnectionTag connection={connection} />
      </div>

      <div role="tablist" className="mx-3.5 mb-1.5 grid grid-cols-3 rounded-[7px] bg-hover p-[3px]">
        {TABS.map((item) => (
          <button
            key={item.id}
            type="button"
            role="tab"
            aria-selected={tab === item.id}
            onClick={() => setTab(item.id)}
            className={cn(
              "flex items-center justify-center gap-1.5 rounded-[5px] py-[5px] text-[12.5px] text-muted",
              tab === item.id && "bg-bg font-semibold text-text shadow-[0_1px_2px_rgb(0_0_0/10%)]",
            )}
          >
            {item.label}
            {item.id === "messages" && unread > 0 && <Badge count={unread} />}
          </button>
        ))}
      </div>

      <nav className="min-h-0 flex-1 overflow-y-auto py-1.5">
        {tab === "messages" &&
          (conversations.length === 0 ? (
            <Empty>还没有会话。新建一个 agent 后，和它的私聊会出现在这里。</Empty>
          ) : (
            conversations.map((conversation) => {
              const preview = conversationPreview(conversation, agents);
              const agent =
                conversation.kind === "direct" ? agents.find((a) => a.roomId === conversation.roomId) : undefined;
              return (
                <Item
                  key={conversation.roomId}
                  active={conversation.roomId === selectedRoomId}
                  onClick={() => onSelect(conversation.roomId)}
                  avatar={
                    agent ? (
                      <AgentAvatar
                        name={agent.displayName}
                        handle={agent.handle}
                        status={statusView(agent.status).tone}
                        size={38}
                      />
                    ) : (
                      <GroupAvatar members={membersOf(conversation.agentIds)} size={38} />
                    )
                  }
                  title={conversation.kind === "group" ? `# ${conversation.name}` : conversation.name}
                  aside={formatListTime(conversation.activeAt, now)}
                  subtitle={
                    <span className={cn(preview.kind === "working" && "font-mono text-[11.5px] text-accent")}>
                      {preview.text}
                    </span>
                  }
                  badge={conversation.unread > 0 ? <Badge count={conversation.unread} tone="accent" /> : undefined}
                />
              );
            })
          ))}

        {tab === "groups" &&
          (groups.length === 0 ? (
            <Empty>还没有群聊。用右上角的“＋ 新建”建一个，让几个 agent 一起协作。</Empty>
          ) : (
            groups.map((group) => {
              const members = groupMembers(group, agents);
              return (
                <Item
                  key={group.id}
                  active={group.id === selectedRoomId}
                  onClick={() => onSelect(group.id)}
                  avatar={<GroupAvatar members={membersOf(group.agentIds)} size={38} />}
                  title={`# ${group.name}`}
                  subtitle={members.map((agent) => agent.displayName).join("、") || "还没有成员"}
                />
              );
            })
          ))}

        {tab === "contacts" &&
          (agents.length === 0 ? (
            <Empty>还没有 agent。用右上角的“＋ 新建”建一个。</Empty>
          ) : (
            agents.map((agent) => (
              <Item
                key={agent.id}
                active={agent.roomId === selectedRoomId}
                onClick={() => onSelect(agent.roomId)}
                avatar={
                  <AgentAvatar
                    name={agent.displayName}
                    handle={agent.handle}
                    status={statusView(agent.status).tone}
                    size={38}
                  />
                }
                title={agent.displayName}
                titleNote={`@${agent.handle}`}
                subtitle={`${agent.persona.split("\n")[0]} · ${agent.model}`}
              />
            ))
          ))}
      </nav>
    </aside>
  );
}

/** 界面与 Server 的 SSE 连接状态。 */
function ConnectionTag({ connection }: { connection: Connection }) {
  if (connection === "reconnecting") {
    return (
      <StatusTag tone="reconnecting" className="text-[11px]">
        正在重新连接…
      </StatusTag>
    );
  }
  if (connection === "connected") {
    return (
      <StatusTag tone="working" className="text-[11px]">
        已连接
      </StatusTag>
    );
  }
  return (
    <StatusTag tone="idle" className="text-[11px]">
      连接中…
    </StatusTag>
  );
}

function Item({
  active,
  onClick,
  avatar,
  title,
  titleNote,
  aside,
  subtitle,
  badge,
}: {
  active: boolean;
  onClick(): void;
  avatar: React.ReactNode;
  title: string;
  titleNote?: string;
  aside?: string;
  subtitle: React.ReactNode;
  badge?: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "mx-1.5 flex w-[calc(100%-12px)] items-center gap-[11px] rounded-lg py-[9px] pr-3 pl-2.5 text-left hover:bg-hover",
        active && "bg-bg shadow-[0_0_0_1px_var(--line-strong)] hover:bg-bg",
      )}
    >
      {avatar}
      <span className="min-w-0 flex-1">
        <span className="flex items-baseline gap-1.5">
          <span className="truncate text-[13.5px] font-semibold">{title}</span>
          {titleNote && <span className="truncate font-mono text-[11px] text-faint">{titleNote}</span>}
          {aside && <span className="ml-auto flex-none font-mono text-[11px] text-faint">{aside}</span>}
        </span>
        <span className="mt-px flex items-center gap-1.5">
          <span className="min-w-0 flex-1 truncate text-[12.5px] text-muted">{subtitle}</span>
          {badge}
        </span>
      </span>
    </button>
  );
}

function Badge({ count, tone = "danger" }: { count: number; tone?: "danger" | "accent" }) {
  return (
    <span
      className={cn(
        "h-4 min-w-4 flex-none rounded-full px-1 text-center font-mono text-[10px] leading-4 font-semibold",
        tone === "danger" ? "bg-danger text-white" : "bg-accent text-accent-fg",
      )}
    >
      {unreadLabel(count)}
    </span>
  );
}

function Empty({ children }: { children: string }) {
  return <p className="px-[22px] py-6 text-[12.5px] leading-relaxed text-faint">{children}</p>;
}
