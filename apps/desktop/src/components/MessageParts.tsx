import type {
  DesktopAgent as Agent,
  MessageView,
  Participant,
  RoomId,
  RoomMessage,
  ThreadSummary,
} from "@crew/protocol";
import { useEffect, useRef, useState } from "react";
import { cn } from "../lib/cn";
import { canSend, shouldSend } from "../lib/composer";
import { newestSeq } from "../lib/messages";
import { useMarkRead, useRun } from "../lib/queries";
import { formatDuration, liveView } from "../lib/runs";
import { formatMessageTime } from "../lib/time";
import { AgentAvatar, UserAvatar } from "./Avatar";
import { Markdown } from "./Markdown";
import { Button } from "./ui/button";

// 房间与讨论串共用的部件：一条消息、Agent 正在回复的实时活动、输入框，以及打开期间推进已读位置。

/** 一个参与者的头像。 */
export function ParticipantAvatar({ who, size = 36 }: { who: Participant; size?: number }) {
  return who.kind === "user" ? (
    <UserAvatar size={size} />
  ) : (
    <AgentAvatar name={who.displayName} handle={who.handle ?? who.id} size={size} ring="var(--bg)" />
  );
}

/**
 * 一条消息。群聊里的消息可以带着它的讨论串摘要；`onOpenThread` 存在时，鼠标移上去出现工具条。
 * `highlighted` 是右栏正打开着它的讨论串。
 */
export function MessageItem({
  message,
  now,
  handles,
  onOpenRun,
  thread,
  onOpenThread,
  highlighted = false,
}: {
  message: MessageView & Partial<Pick<RoomMessage, "runId" | "heldBefore">>;
  now: Date;
  handles: ReadonlySet<string>;
  onOpenRun(runId: string): void;
  thread?: ThreadSummary;
  onOpenThread?(message: MessageView): void;
  highlighted?: boolean;
}) {
  const { author, runId } = message;
  const heldBefore = message.heldBefore ?? 0;
  return (
    <article
      className={cn(
        "group relative -mx-2.5 mb-3 flex gap-3 rounded-xl px-2.5 py-2",
        highlighted ? "bg-accent-soft" : "hover:bg-hover/60",
      )}
    >
      <ParticipantAvatar who={author} />
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline gap-2 font-mono text-[11px] text-faint">
          <b className="font-sans text-[13px] font-semibold text-text">
            {author.kind === "user" ? "你" : author.displayName}
          </b>
          {author.handle && <span>@{author.handle}</span>}
          <time dateTime={message.createdAt}>{formatMessageTime(message.createdAt, now)}</time>
        </div>
        <Markdown handles={handles}>{message.body}</Markdown>
        {runId && (
          <div className="mt-1 flex gap-3 font-mono text-[11px] text-faint">
            {heldBefore > 0 && (
              <span className="text-warn">
                ↻ 看到新消息后改写了回复{heldBefore > 1 ? `（被拦下 ${heldBefore} 次）` : ""}
              </span>
            )}
            <button
              type="button"
              onClick={() => onOpenRun(runId)}
              className="underline decoration-dotted underline-offset-[3px] hover:text-text"
            >
              这一轮
            </button>
          </div>
        )}
        {thread && onOpenThread && <ThreadChip thread={thread} now={now} onClick={() => onOpenThread(message)} />}
      </div>
      {onOpenThread && (
        <div className="absolute -top-3.5 right-2.5 hidden gap-0.5 rounded-[9px] border border-line bg-raised p-0.5 text-xs text-muted shadow-pop group-hover:flex">
          <ToolbarButton onClick={() => onOpenThread(message)}>↳ {thread ? "查看讨论串" : "开讨论串"}</ToolbarButton>
          <ToolbarButton onClick={() => void navigator.clipboard.writeText(message.body)}>复制</ToolbarButton>
        </div>
      )}
    </article>
  );
}

function ToolbarButton({ onClick, children }: { onClick(): void; children: React.ReactNode }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="rounded-md px-2 py-[3px] whitespace-nowrap hover:bg-hover hover:text-text"
    >
      {children}
    </button>
  );
}

/** 宿主消息下面的讨论串摘要：参与者、回复数、最后回复时间与新回复数。 */
export function ThreadChip({ thread, now, onClick }: { thread: ThreadSummary; now: Date; onClick(): void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="mt-1.5 inline-flex max-w-full items-center gap-2 overflow-hidden rounded-[9px] border border-line bg-raised py-1 pr-2.5 pl-1.5 text-xs whitespace-nowrap text-muted hover:border-line-strong"
    >
      <AvatarStack people={thread.participants} />
      <b className="flex-none font-semibold text-accent">{thread.replies} 条回复</b>
      {thread.lastReplyAt && <span className="truncate">最后回复 {formatMessageTime(thread.lastReplyAt, now)}</span>}
      {thread.unread > 0 && (
        <span className="flex-none rounded-lg bg-accent px-1.5 text-[10.5px] leading-4 font-semibold text-accent-fg">
          {thread.unread} 条新
        </span>
      )}
    </button>
  );
}

/** 叠在一起的小头像。 */
export function AvatarStack({ people, size = 18 }: { people: Participant[]; size?: number }) {
  return (
    <span className="flex">
      {people.map((who, index) => (
        <span
          key={`${who.kind}:${who.id}`}
          // 前面的压在后面的上面：第一个发言的人完整可见。
          style={{ zIndex: people.length - index }}
          className={cn("relative rounded-[6px] shadow-[0_0_0_2px_var(--raised)]", index > 0 && "-ml-1")}
        >
          <ParticipantAvatar who={who} size={size} />
        </span>
      ))}
    </span>
  );
}

/**
 * Agent 正在这里跑一轮：实时列出最近几次工具调用与“思考中”，点击打开这一轮的记录。
 * 只在唤醒这一轮的房间里显示：讨论串唤醒的一轮，Agent 在群聊里的状态也是回复中，但活动框只出现在讨论串里。
 */
export function LiveActivity({
  agent,
  runId,
  roomId,
  onOpenRun,
}: {
  agent: Agent;
  runId: string;
  roomId: RoomId;
  onOpenRun(runId: string): void;
}) {
  const { data: run } = useRun(runId);
  const now = useNow(1000);
  if (!run?.triggers.some((trigger) => trigger.roomId === roomId)) return null;
  const view = liveView(run);
  const elapsed = formatDuration(now - new Date(run.startedAt).getTime());
  return (
    <div className="mb-[18px] flex gap-3">
      <AgentAvatar name={agent.displayName} handle={agent.handle} size={22} className="mt-2 ml-[7px]" />
      <button
        type="button"
        onClick={() => onOpenRun(runId)}
        className="w-full max-w-[520px] rounded-lg border border-l-2 border-line border-l-accent bg-raised px-3 py-2 text-left font-mono"
      >
        <span className="flex items-center gap-2 text-[12.5px] text-muted">
          {agent.displayName} 正在回复
          <span className="cursor-blink inline-block h-[13px] w-[7px] bg-accent" />
          <span className="ml-auto text-[11px] text-faint">
            已运行 {elapsed}
            {view.steps > 0 ? ` · 第 ${view.steps} 步` : ""}
          </span>
        </span>
        {(view.recent.length > 0 || view.thinking) && (
          <ol className="mt-1.5 text-[12px] leading-[1.75] text-muted">
            {view.recent.map((step) => (
              <li key={step.seq} className="flex gap-2">
                <span className="w-3.5 flex-none text-center text-faint">{step.mark}</span>
                <span className="min-w-0 flex-1 truncate">{step.text}</span>
                <span className="flex-none text-faint">{step.duration}</span>
              </li>
            ))}
            {view.thinking && (
              <li className="flex gap-2 text-text">
                <span className="w-3.5 flex-none text-center text-faint">…</span>思考中
              </li>
            )}
          </ol>
        )}
      </button>
    </div>
  );
}

/** 每隔 `intervalMs` 更新一次的当前时间，用来显示已经运行了多久。 */
function useNow(intervalMs: number): number {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), intervalMs);
    return () => clearInterval(timer);
  }, [intervalMs]);
  return now;
}

/**
 * 输入框：Enter 发送，Shift+Enter 换行。`send` 成功后清空草稿；失败时保留草稿并显示原因。
 */
export function Composer({
  placeholder,
  send,
  className,
}: {
  placeholder: string;
  send: {
    mutate(body: string, options: { onSuccess(): void }): void;
    isPending: boolean;
    error: Error | null;
    reset(): void;
  };
  className?: string;
}) {
  const [draft, setDraft] = useState("");

  const submit = () => {
    if (!canSend(draft) || send.isPending) return;
    send.mutate(draft, { onSuccess: () => setDraft("") });
  };

  return (
    <div className={className}>
      <div
        className={
          send.error
            ? "rounded-xl border border-danger bg-raised"
            : "rounded-xl border border-line-strong bg-raised shadow-card focus-within:border-accent"
        }
      >
        <textarea
          value={draft}
          onChange={(event) => {
            setDraft(event.target.value);
            if (send.error) send.reset();
          }}
          onKeyDown={(event) => {
            if (shouldSend({ ...event, isComposing: event.nativeEvent.isComposing })) {
              event.preventDefault();
              submit();
            }
          }}
          placeholder={placeholder}
          rows={2}
          className="block max-h-60 min-h-[52px] w-full resize-none bg-transparent px-3.5 pt-3 pb-1 text-[15px] leading-[1.7] text-text outline-none placeholder:text-faint [field-sizing:content]"
        />
        <div className="flex items-center gap-2.5 pt-1.5 pr-2 pb-2 pl-3.5 font-mono text-[11px] text-faint">
          <span>Markdown</span>
          <span>
            <Kbd>⏎</Kbd> 发送　<Kbd>⇧</Kbd> <Kbd>⏎</Kbd> 换行
          </span>
          <Button
            variant="primary"
            size="sm"
            className="ml-auto"
            disabled={!canSend(draft) || send.isPending}
            onClick={submit}
          >
            发送
          </Button>
        </div>
      </div>
      {send.error && <p className="mt-1.5 text-xs text-danger">发送失败：{send.error.message}。草稿已保留。</p>}
    </div>
  );
}

function Kbd({ children }: { children: string }) {
  return (
    <kbd className="rounded-[3px] border border-b-2 border-line-strong px-1 font-mono text-[11px] text-muted">
      {children}
    </kbd>
  );
}

/**
 * 房间或讨论串开着、窗口在前台时，把用户的已读位置推进到最新一条：打开时一次，之后每来新消息、
 * 窗口回到前台时再推进。窗口在后台时到达的消息保持未读，侧栏显示未读数。
 */
export function useMarkReadWhileOpen(roomId: RoomId, messages: readonly RoomMessage[] | undefined) {
  const markRead = useMarkRead(roomId);
  const marked = useRef(0);
  const newest = messages ? newestSeq(messages) : 0;
  const { mutate } = markRead;

  useEffect(() => {
    const mark = () => {
      if (newest <= marked.current || document.visibilityState !== "visible" || !document.hasFocus()) return;
      const previous = marked.current;
      marked.current = newest;
      // 请求失败时退回去，窗口下次回到前台时重试。
      mutate(newest, {
        onError: () => {
          marked.current = previous;
        },
      });
    };
    mark();
    window.addEventListener("focus", mark);
    document.addEventListener("visibilitychange", mark);
    return () => {
      window.removeEventListener("focus", mark);
      document.removeEventListener("visibilitychange", mark);
    };
  }, [newest, mutate]);
}
