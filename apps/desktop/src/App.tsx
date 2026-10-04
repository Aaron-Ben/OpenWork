import type { AgentId, DesktopAgent, DesktopGroup, RoomId } from "@crew/protocol";
import { QueryClientProvider } from "@tanstack/react-query";
import { useState } from "react";
import { type ChatRoom, ChatView } from "./components/ChatView";
import { AddMembersDialog, NewGroupDialog } from "./components/GroupDialogs";
import { NewAgentDialog } from "./components/NewAgentDialog";
import { Sidebar } from "./components/Sidebar";
import { Button } from "./components/ui/button";
import { useServerEvents } from "./lib/events";
import { groupMembers } from "./lib/new-group";
import { createQueryClient, useAgents, useGroups } from "./lib/queries";

const queryClient = createQueryClient();

export function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <Workbench />
    </QueryClientProvider>
  );
}

/** 当前打开的房间：某个 Agent 的私聊，或某个群聊。 */
export type Selection = { kind: "agent"; id: AgentId } | { kind: "group"; id: RoomId };

function Workbench() {
  const connection = useServerEvents();
  const agents = useAgents();
  const groups = useGroups();
  const [selection, setSelection] = useState<Selection>();
  const [creatingAgent, setCreatingAgent] = useState(false);
  const [creatingGroup, setCreatingGroup] = useState(false);
  const [addingMembers, setAddingMembers] = useState(false);

  const agentList = agents.data ?? [];
  const groupList = groups.data ?? [];
  const room = openRoom(selection, agentList, groupList);

  return (
    <div className="flex h-full">
      <Sidebar
        agents={agentList}
        groups={groupList}
        selected={room && selectionOf(room)}
        onSelect={setSelection}
        onCreateAgent={() => setCreatingAgent(true)}
        onCreateGroup={() => setCreatingGroup(true)}
        connection={connection}
      />
      {room ? (
        <ChatView room={room} onAddMembers={() => setAddingMembers(true)} />
      ) : (
        <main className="flex flex-1 flex-col">
          <div className="drag h-[52px] flex-none" />
          <div className="grid flex-1 place-items-center px-7 pb-[52px] text-center">
            {agents.error ? (
              <p className="text-sm text-danger">读取 agent 列表失败：{agents.error.message}</p>
            ) : agents.isSuccess ? (
              <div className="max-w-[380px]">
                <h2 className="font-mono text-[15px] font-semibold">还没有 agent</h2>
                <p className="mt-2 mb-[18px] text-sm leading-relaxed text-muted">
                  新建一个 agent。它在本机沙箱里运行 OpenCode，用你已登录的账号调用模型。
                </p>
                <Button variant="primary" onClick={() => setCreatingAgent(true)}>
                  ＋ 新建 agent
                </Button>
              </div>
            ) : null}
          </div>
        </main>
      )}
      <NewAgentDialog
        open={creatingAgent}
        onOpenChange={setCreatingAgent}
        onCreated={(id) => {
          setSelection({ kind: "agent", id });
          setCreatingAgent(false);
        }}
      />
      <NewGroupDialog
        open={creatingGroup}
        onOpenChange={setCreatingGroup}
        agents={agentList}
        onCreated={(id) => {
          setSelection({ kind: "group", id });
          setCreatingGroup(false);
        }}
      />
      {room?.kind === "group" && (
        <AddMembersDialog
          open={addingMembers}
          onOpenChange={setAddingMembers}
          group={room.group}
          agents={agentList}
          onAdded={() => setAddingMembers(false)}
        />
      )}
    </div>
  );
}

function selectionOf(room: ChatRoom): Selection {
  return room.kind === "group" ? { kind: "group", id: room.group.id } : { kind: "agent", id: room.agent.id };
}

/**
 * 选中的房间。选中的群聊或 Agent 已不在列表里，或者还没选过时，打开第一个 Agent 的私聊；
 * 没有 Agent 时打开第一个群聊。
 */
function openRoom(
  selection: Selection | undefined,
  agents: DesktopAgent[],
  groups: DesktopGroup[],
): ChatRoom | undefined {
  if (selection?.kind === "group") {
    const group = groups.find((candidate) => candidate.id === selection.id);
    if (group) return { kind: "group", group, members: groupMembers(group, agents) };
  }
  const agent = agents.find((candidate) => selection?.kind === "agent" && candidate.id === selection.id) ?? agents[0];
  if (agent) return { kind: "direct", agent };
  const group = groups[0];
  return group && { kind: "group", group, members: groupMembers(group, agents) };
}
