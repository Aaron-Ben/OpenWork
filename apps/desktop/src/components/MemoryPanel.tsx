import type { DesktopAgent as Agent } from "@crew/protocol";
import { useMemo } from "react";
import { useMemory } from "../lib/queries";
import { formatMessageTime } from "../lib/time";
import { Markdown } from "./Markdown";

/**
 * 右栏里的记忆：私聊对象的 `MEMORY.md`，只读。Agent 自己维护这个文件，开新会话时先读它；
 * 这里让用户看到它记住了什么。每一轮结束时（Agent 状态变化）重新读取。
 */
export function MemoryPanel({ agent, agents, expanded }: { agent: Agent; agents: Agent[]; expanded: boolean }) {
  const memory = useMemory(agent.id, true);
  const handles = useMemo(() => new Set(agents.map((candidate) => candidate.handle)), [agents]);
  const now = new Date();
  return (
    <div className="min-h-0 flex-1 overflow-y-auto px-4 pb-4">
      <div className={expanded ? "mx-auto w-full max-w-[720px]" : ""}>
        <p className="mb-3 rounded-[10px] bg-panel px-3 py-2 text-[12px] leading-relaxed text-muted">
          {agent.displayName} 自己维护这份文件，开新会话时先读它。位置：
          <span className="selectable font-mono text-[11px]"> ~/.crew/agents/…/work/MEMORY.md</span>
        </p>
        {memory.error && <p className="text-xs text-danger">读取记忆失败：{memory.error.message}</p>}
        {memory.data === null && (
          <p className="py-6 text-[12.5px] text-faint">还没有记忆文件。agent 第一次运行时会建好。</p>
        )}
        {memory.data && (
          <>
            <div className="mb-2 flex gap-3 font-mono text-[11px] text-faint">
              <span>{(memory.data.bytes / 1024).toFixed(1)} KB</span>
              <span>更新于 {formatMessageTime(memory.data.modifiedAt, now)}</span>
              {memory.data.truncated && <span className="text-warn">太长，只显示了开头</span>}
            </div>
            <Markdown handles={handles}>{memory.data.content}</Markdown>
          </>
        )}
      </div>
    </div>
  );
}
