import type { AgentId } from "@crew/protocol";
import { QueryClientProvider } from "@tanstack/react-query";
import { useState } from "react";
import { ChatView } from "./components/ChatView";
import { NewAgentDialog } from "./components/NewAgentDialog";
import { Sidebar } from "./components/Sidebar";
import { Button } from "./components/ui/button";
import { useServerEvents } from "./lib/events";
import { createQueryClient, useAgents } from "./lib/queries";

const queryClient = createQueryClient();

export function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <Workbench />
    </QueryClientProvider>
  );
}

function Workbench() {
  const connection = useServerEvents();
  const agents = useAgents();
  const [selectedId, setSelectedId] = useState<AgentId>();
  const [creating, setCreating] = useState(false);

  const list = agents.data ?? [];
  // 默认选第一个；选中的 Agent 不在列表里时同样回到第一个。
  const selected = list.find((agent) => agent.id === selectedId) ?? list[0];

  return (
    <div className="flex h-full">
      <Sidebar
        agents={list}
        selectedId={selected?.id}
        onSelect={setSelectedId}
        onCreate={() => setCreating(true)}
        connection={connection}
      />
      {selected ? (
        <ChatView agent={selected} />
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
                <Button variant="primary" onClick={() => setCreating(true)}>
                  ＋ 新建 agent
                </Button>
              </div>
            ) : null}
          </div>
        </main>
      )}
      <NewAgentDialog
        open={creating}
        onOpenChange={setCreating}
        onCreated={(id) => {
          setSelectedId(id);
          setCreating(false);
        }}
      />
    </div>
  );
}
