import type { DesktopAgent, DesktopGroup, RoomId } from "@crew/protocol";
import { QueryClientProvider } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { type ChatRoom, ChatView } from "./components/ChatView";
import { AddMembersDialog, JoinGroupsDialog, NewGroupDialog } from "./components/GroupDialogs";
import { NewAgentDialog } from "./components/NewAgentDialog";
import { Sidebar } from "./components/Sidebar";
import { Button } from "./components/ui/button";
import { useServerEvents } from "./lib/events";
import { groupMembers } from "./lib/new-group";
import { createQueryClient, useAgents, useConversations, useGroups } from "./lib/queries";

const queryClient = createQueryClient();

export function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <Workbench />
    </QueryClientProvider>
  );
}

type Dialog = "agent" | "group" | "members" | "join";

function Workbench() {
  const connection = useServerEvents();
  const agents = useAgents();
  const groups = useGroups();
  const conversations = useConversations();
  const [selectedId, setSelectedId] = useState<RoomId>();
  const [dialog, setDialog] = useState<Dialog>();
  /** 侧栏收起。侧栏顶部的按钮收起，聊天顶栏的按钮或 ⌘\ 展开。 */
  const [sidebarHidden, setSidebarHidden] = useState(false);

  const agentList = agents.data ?? [];
  const groupList = groups.data ?? [];
  const conversationList = conversations.data ?? [];
  const loaded = agents.isSuccess && groups.isSuccess && conversations.isSuccess;
  const firstRoomId = conversationList[0]?.roomId;

  // 三份列表都到齐后，默认打开最近活跃的会话，并且只定这一次：会话列表随新消息重新排序，
  // 默认房间要是跟着列表第一项走，别的房间来了消息就会把正在看的房间换掉、丢掉草稿，还会把它记为已读。
  useEffect(() => {
    if (selectedId === undefined && loaded && firstRoomId) setSelectedId(firstRoomId);
  }, [selectedId, loaded, firstRoomId]);

  const room = openRoom(selectedId, agentList, groupList);
  // 没有打开的聊天时侧栏总是显示，收起侧栏也就无从谈起：按钮不显示，⌘\ 不生效，免得打开聊天时侧栏突然消失。
  const canHide = room !== undefined;

  useEffect(() => {
    if (!canHide) return;
    const toggle = (event: KeyboardEvent) => {
      if (event.metaKey && event.key === "\\") {
        event.preventDefault();
        setSidebarHidden((hidden) => !hidden);
      }
    };
    window.addEventListener("keydown", toggle);
    return () => window.removeEventListener("keydown", toggle);
  }, [canHide]);
  const roomId = room && (room.kind === "group" ? room.group.id : room.agent.roomId);

  const close = (open: boolean) => {
    if (!open) setDialog(undefined);
  };

  return (
    <div className="flex h-full">
      {/* 没有打开的聊天时侧栏总是显示：那时没有别的地方能把它找回来。 */}
      {!(sidebarHidden && room) && (
        <Sidebar
          agents={agentList}
          groups={groupList}
          conversations={conversationList}
          selectedRoomId={roomId}
          onSelect={setSelectedId}
          onCreateAgent={() => setDialog("agent")}
          onCreateGroup={() => setDialog("group")}
          connection={connection}
          onHide={canHide ? () => setSidebarHidden(true) : undefined}
        />
      )}
      {room ? (
        <ChatView
          room={room}
          agents={agentList}
          sidebarHidden={sidebarHidden}
          onShowSidebar={() => setSidebarHidden(false)}
          onAddMembers={() => setDialog("members")}
          onJoinGroups={() => setDialog("join")}
        />
      ) : (
        <Welcome
          ready={loaded && agentList.length === 0}
          error={agents.error?.message}
          onCreate={() => setDialog("agent")}
        />
      )}
      <NewAgentDialog
        open={dialog === "agent"}
        onOpenChange={close}
        onCreated={(agent) => {
          setSelectedId(agent.roomId);
          setDialog(undefined);
        }}
      />
      <NewGroupDialog
        open={dialog === "group"}
        onOpenChange={close}
        agents={agentList}
        onCreated={(id) => {
          setSelectedId(id);
          setDialog(undefined);
        }}
      />
      {room?.kind === "group" && (
        <AddMembersDialog
          open={dialog === "members"}
          onOpenChange={close}
          group={room.group}
          agents={agentList}
          onAdded={() => setDialog(undefined)}
        />
      )}
      {room?.kind === "direct" && (
        <JoinGroupsDialog
          open={dialog === "join"}
          onOpenChange={close}
          agent={room.agent}
          groups={groupList}
          onJoined={() => setDialog(undefined)}
        />
      )}
    </div>
  );
}

/** 一个房间 ID 对应的聊天：某个群聊，或某个 Agent 的私聊。都找不到时返回 undefined。 */
function openRoom(roomId: RoomId | undefined, agents: DesktopAgent[], groups: DesktopGroup[]): ChatRoom | undefined {
  if (!roomId) return undefined;
  const group = groups.find((candidate) => candidate.id === roomId);
  if (group) return { kind: "group", group, members: groupMembers(group, agents) };
  const agent = agents.find((candidate) => candidate.roomId === roomId);
  return agent && { kind: "direct", agent };
}

/** 还没有 Agent 时的右侧：三步引导。列表还在读取、或默认房间还没定下时是空白。 */
function Welcome({ ready, error, onCreate }: { ready: boolean; error?: string; onCreate(): void }) {
  return (
    <main className="flex flex-1 flex-col">
      <div className="drag h-[52px] flex-none" />
      <div className="grid flex-1 place-items-center px-7 pb-[52px]">
        {error ? (
          <p className="text-sm text-danger">读取 agent 列表失败：{error}</p>
        ) : !ready ? null : (
          <div className="w-full max-w-[460px]">
            <h2 className="text-xl font-semibold">开始使用 Crew</h2>
            <p className="mt-1.5 mb-[22px] text-[13.5px] text-muted">
              agent 在本机沙箱里运行 OpenCode，用你已登录的账号调用模型。
            </p>
            <Step n={1} title="新建 agent" detail="名字、handle、人设与模型">
              <Button variant="primary" onClick={onCreate}>
                新建
              </Button>
            </Step>
            <Step n={2} title="和它私聊" detail="发一条消息，看它在沙箱里回复" />
            <Step n={3} title="建一个群聊" detail="几个 agent 一起协作，用 @handle 点名" />
          </div>
        )}
      </div>
    </main>
  );
}

function Step({
  n,
  title,
  detail,
  children,
}: {
  n: number;
  title: string;
  detail: string;
  children?: React.ReactNode;
}) {
  return (
    <div className="mb-2.5 flex items-center gap-3.5 rounded-[10px] border border-line bg-raised px-4 py-3.5">
      <span className="grid size-[26px] flex-none place-items-center rounded-full border border-line-strong font-mono text-xs font-semibold text-muted">
        {n}
      </span>
      <span className="min-w-0 flex-1">
        <b className="block text-[13.5px]">{title}</b>
        <span className="text-[12.5px] text-muted">{detail}</span>
      </span>
      {children}
    </div>
  );
}
