import { useLayoutEffect, useRef, useState } from "react";
import { canSend, shouldSend } from "../lib/composer";
import { useMessages, useSendMessage } from "../lib/queries";
import { isNearBottom } from "../lib/scroll";
import type { Agent, Message } from "../lib/server";
import { statusView } from "../lib/status";
import { formatMessageTime } from "../lib/time";
import { Markdown } from "./Markdown";
import { StatusTag } from "./StatusTag";
import { Button } from "./ui/button";

/** 右侧：与一个 Agent 的私聊。 */
export function ChatView({ agent }: { agent: Agent }) {
  const status = statusView(agent.status);
  return (
    <main className="flex min-w-0 flex-1 flex-col">
      <header className="drag flex h-[52px] flex-none items-center gap-3 border-b border-line px-7 font-mono">
        <h1 className="truncate text-sm font-semibold">{agent.displayName}</h1>
        <span className="truncate text-xs text-muted">{agent.model}</span>
        <StatusTag tone={status.tone} className="ml-auto flex-none">
          {status.label}
        </StatusTag>
      </header>
      {/* 换 Agent 时重建消息列表与输入框：滚动位置与草稿属于各自的房间。 */}
      <MessageList key={agent.id} agent={agent} />
      <Composer key={`composer-${agent.id}`} agent={agent} />
    </main>
  );
}

function MessageList({ agent }: { agent: Agent }) {
  const { data: messages, error, isPending } = useMessages(agent.roomId);
  const scroller = useRef<HTMLDivElement>(null);
  const stickToBottom = useRef(true);

  // 新消息或状态变化后，原本停在底部就继续贴着底部。
  // biome-ignore lint/correctness/useExhaustiveDependencies: 两者是触发滚动的条件，回调里不需要读取它们。
  useLayoutEffect(() => {
    const element = scroller.current;
    if (element && stickToBottom.current) element.scrollTop = element.scrollHeight;
  }, [messages, agent.status]);

  if (isPending) return <div className="flex-1" />;
  if (error) return <p className="flex-1 px-7 py-6 text-sm text-danger">读取消息失败：{error.message}</p>;
  if (messages.length === 0) return <EmptyChat agent={agent} />;

  const now = new Date();
  return (
    <div
      ref={scroller}
      className="min-h-0 flex-1 overflow-y-auto"
      onScroll={(event) => {
        stickToBottom.current = isNearBottom(event.currentTarget);
      }}
    >
      <div className="max-w-[736px] px-7 pt-6 pb-2">
        {messages.map((message) => (
          <MessageItem key={message.id} message={message} now={now} />
        ))}
        {agent.status.state === "working" && (
          <div className="mb-[26px]">
            <div className="mb-1 font-mono text-[13px] font-semibold text-accent">{agent.displayName}</div>
            <span className="font-mono text-[13px] text-muted">
              正在回复
              <span className="cursor-blink ml-1 inline-block h-[15px] w-[7px] bg-accent align-[-3px]" />
            </span>
          </div>
        )}
        {agent.status.state === "error" && (
          <div className="mb-[26px] rounded-md border border-danger bg-danger-soft px-3.5 py-3">
            <div className="font-mono text-xs font-semibold text-danger">✕ 上一轮没有完成</div>
            <p className="selectable mt-1 text-sm leading-relaxed">{agent.status.reason}</p>
            <p className="text-xs text-muted">下一条消息到来时会自动重试。</p>
          </div>
        )}
      </div>
    </div>
  );
}

function MessageItem({ message, now }: { message: Message; now: Date }) {
  const isUser = message.author.kind === "user";
  return (
    <article className="mb-[26px]">
      <div className="mb-1 flex items-baseline gap-2.5 font-mono text-xs">
        <b className={isUser ? "text-[13px] font-semibold text-user" : "text-[13px] font-semibold text-accent"}>
          {isUser ? "你" : message.author.displayName}
        </b>
        <time className="text-faint" dateTime={message.createdAt}>
          {formatMessageTime(message.createdAt, now)}
        </time>
      </div>
      <Markdown>{message.body}</Markdown>
    </article>
  );
}

function EmptyChat({ agent }: { agent: Agent }) {
  return (
    <div className="grid flex-1 place-items-center px-7">
      <div className="w-full max-w-[380px] text-center">
        <h2 className="font-mono text-[15px] font-semibold">和 {agent.displayName} 的对话从这里开始</h2>
        <div className="selectable mt-4 rounded-md border border-line px-3.5 py-3 text-left text-[13px] leading-relaxed whitespace-pre-wrap text-muted">
          <div className="mb-1 font-mono text-[11px] text-faint">人设</div>
          {agent.persona}
        </div>
      </div>
    </div>
  );
}

function Composer({ agent }: { agent: Agent }) {
  const [draft, setDraft] = useState("");
  const send = useSendMessage(agent.roomId);

  const submit = () => {
    if (!canSend(draft) || send.isPending) return;
    send.mutate(draft, { onSuccess: () => setDraft("") });
  };

  return (
    <div className="max-w-[736px] px-7 pb-[18px]">
      <div
        className={
          send.error
            ? "rounded-md border border-danger bg-raised"
            : "rounded-md border border-line-strong bg-raised focus-within:border-accent"
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
          placeholder={`给 ${agent.displayName} 发消息`}
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
