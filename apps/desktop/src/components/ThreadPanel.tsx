import type { DesktopAgent as Agent, MessageView, RoomId, ThreadSummary } from "@crew/protocol";
import { useLayoutEffect, useMemo, useRef } from "react";
import { cn } from "../lib/cn";
import { useMessages, useSendMessage, useThreads } from "../lib/queries";
import { isNearBottom } from "../lib/scroll";
import { statusIn } from "../lib/status";
import { formatMessageTime } from "../lib/time";
import { Markdown } from "./Markdown";
import {
  Composer,
  LiveActivity,
  MessageItem,
  ParticipantAvatar,
  ThreadChip,
  useMarkReadWhileOpen,
} from "./MessageParts";

/**
 * 右栏里的讨论串。`parent` 为空时列出群聊里的全部讨论串；否则显示挂在 `parent` 下的讨论串：
 * 宿主消息、回复与输入框。讨论串还没有时，第一条回复创建它。
 */
export function ThreadPanel({
  groupId,
  members,
  agents,
  parent,
  expanded,
  onOpen,
  onOpenRun,
}: {
  groupId: RoomId;
  /** 群聊的 Agent 成员，带着它们的状态：正在讨论串里回复的显示实时活动。 */
  members: Agent[];
  /** 全部 Agent：消息里的 @handle 按它们高亮。 */
  agents: Agent[];
  parent: MessageView | undefined;
  expanded: boolean;
  onOpen(parent: MessageView): void;
  onOpenRun(runId: string): void;
}) {
  const threads = useThreads(groupId);
  const handles = useMemo(() => new Set(agents.map((agent) => agent.handle)), [agents]);
  if (!parent) return <ThreadList threads={threads.data} error={threads.error} expanded={expanded} onOpen={onOpen} />;
  const thread = threads.data?.find((candidate) => candidate.parent.id === parent.id);
  return (
    <ThreadView
      key={parent.id}
      groupId={groupId}
      members={members}
      handles={handles}
      parent={parent}
      thread={thread}
      expanded={expanded}
      onOpenRun={onOpenRun}
    />
  );
}

/** 放大后正文居中，最宽 720px，不拉成很长的行。 */
const column = (expanded: boolean) => (expanded ? "mx-auto w-full max-w-[720px]" : "");

function ThreadList({
  threads,
  error,
  expanded,
  onOpen,
}: {
  threads: ThreadSummary[] | undefined;
  error: Error | null;
  expanded: boolean;
  onOpen(parent: MessageView): void;
}) {
  const now = new Date();
  return (
    <div className="min-h-0 flex-1 overflow-y-auto px-3 py-3">
      <div className={column(expanded)}>
        {error && <p className="px-2 py-4 text-xs text-danger">读取讨论串失败：{error.message}</p>}
        {threads?.length === 0 && (
          <p className="px-3 py-6 text-[12.5px] leading-relaxed text-faint">
            还没有讨论串。鼠标移到群里的一条消息上，点“开讨论串”，细节讨论就不会挤进主时间线。
          </p>
        )}
        {[...(threads ?? [])].reverse().map((thread) => (
          <div
            key={thread.id}
            className="mb-1.5 rounded-xl border border-transparent px-3 py-2.5 hover:border-line hover:bg-raised hover:shadow-card"
          >
            <button type="button" className="flex w-full gap-2.5 text-left" onClick={() => onOpen(thread.parent)}>
              <ParticipantAvatar who={thread.parent.author} size={24} />
              <span className="min-w-0 flex-1">
                <span className="flex items-baseline gap-2 text-[11px] text-faint">
                  <b className="text-[12.5px] font-semibold text-text">
                    {thread.parent.author.kind === "user" ? "你" : thread.parent.author.displayName}
                  </b>
                  {formatMessageTime(thread.parent.createdAt, now)}
                </span>
                <span className="line-clamp-2 text-[13px] leading-relaxed text-muted">{thread.parent.body}</span>
              </span>
            </button>
            <div className="pl-[34px]">
              <ThreadChip thread={thread} now={now} onClick={() => onOpen(thread.parent)} />
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

function ThreadView({
  groupId,
  members,
  handles,
  parent,
  thread,
  expanded,
  onOpenRun,
}: {
  groupId: RoomId;
  members: Agent[];
  handles: ReadonlySet<string>;
  parent: MessageView;
  thread: ThreadSummary | undefined;
  expanded: boolean;
  onOpenRun(runId: string): void;
}) {
  const send = useSendMessage(groupId);
  const now = new Date();
  return (
    <>
      <div className="min-h-0 flex-1 overflow-y-auto px-4 py-4">
        <div className={column(expanded)}>
          <div className="rounded-xl border border-line bg-raised px-3.5 py-3 shadow-card">
            <div className="flex items-baseline gap-2 text-[11px] text-faint">
              <b className="text-[13px] font-semibold text-text">
                {parent.author.kind === "user" ? "你" : parent.author.displayName}
              </b>
              {formatMessageTime(parent.createdAt, now)}
            </div>
            <Markdown handles={handles}>{parent.body}</Markdown>
          </div>
          {thread ? (
            <ThreadReplies
              threadId={thread.id}
              thread={thread}
              members={members}
              handles={handles}
              onOpenRun={onOpenRun}
            />
          ) : (
            <p className="mt-4 text-center text-[12.5px] text-faint">
              还没有回复。你的第一条回复会开出这个讨论串，并唤醒这条消息的作者或你 @ 到的 agent。
            </p>
          )}
        </div>
      </div>
      <Composer
        placeholder="回复讨论串，用 @handle 拉人进来"
        className={cn("px-4 pb-4", column(expanded))}
        send={{
          mutate: (body, options) => send.mutate({ body, threadOf: parent.id }, options),
          isPending: send.isPending,
          error: send.error,
          reset: send.reset,
        }}
      />
    </>
  );
}

function ThreadReplies({
  threadId,
  thread,
  members,
  handles,
  onOpenRun,
}: {
  threadId: RoomId;
  thread: ThreadSummary;
  members: Agent[];
  handles: ReadonlySet<string>;
  onOpenRun(runId: string): void;
}) {
  const { data: messages, error } = useMessages(threadId);
  useMarkReadWhileOpen(threadId, messages);
  const working = members
    .map((agent) => ({ ...agent, status: statusIn(agent.status, threadId) }))
    .filter((agent) => agent.status.state === "working");
  const end = useRef<HTMLDivElement>(null);
  const following = useRef(true);
  const count = messages?.length ?? 0;
  const workingKey = working.map((agent) => agent.id).join(",");

  // 新回复或有人开始回复时，跟到最下面；用户往上翻看时不打断。
  // biome-ignore lint/correctness/useExhaustiveDependencies: 两者是触发滚动的条件，回调里不需要读取它们。
  useLayoutEffect(() => {
    if (following.current) end.current?.scrollIntoView({ block: "end" });
  }, [count, workingKey]);

  const now = new Date();
  const followers = thread.participants.filter((who) => who.kind === "agent").map((who) => who.displayName);
  return (
    <div
      onWheel={(event) => {
        const scroller = event.currentTarget.closest(".overflow-y-auto");
        if (scroller instanceof HTMLElement) following.current = isNearBottom(scroller);
      }}
    >
      <div className="my-3.5 flex items-center gap-2.5 text-[11.5px] text-faint after:h-px after:flex-1 after:bg-line">
        {thread.replies} 条回复{followers.length > 0 ? ` · ${followers.join("、")} 参与` : ""}
      </div>
      {error && <p className="text-xs text-danger">读取回复失败：{error.message}</p>}
      {messages?.map((message) => (
        <MessageItem key={message.id} message={message} now={now} handles={handles} onOpenRun={onOpenRun} />
      ))}
      {working.map((agent) =>
        agent.status.state === "working" ? (
          <LiveActivity
            key={agent.id}
            agent={agent}
            runId={agent.status.runId}
            roomId={threadId}
            onOpenRun={onOpenRun}
          />
        ) : null,
      )}
      <div ref={end} />
    </div>
  );
}
