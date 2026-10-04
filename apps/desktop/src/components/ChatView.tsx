import type { DesktopAgent as Agent, DesktopGroup as Group, RoomMessage as Message, RoomId } from "@crew/protocol";
import { useLayoutEffect, useRef, useState } from "react";
import { canSend, shouldSend } from "../lib/composer";
import { hasOlder } from "../lib/messages";
import { useLoadOlder, useMessages, useSendMessage } from "../lib/queries";
import { isNearBottom } from "../lib/scroll";
import { statusView } from "../lib/status";
import { formatMessageTime } from "../lib/time";
import { Markdown } from "./Markdown";
import { StatusTag } from "./StatusTag";
import { Button } from "./ui/button";

/** 打开的房间：与一个 Agent 的私聊，或一个群聊与其中的 Agent。 */
export type ChatRoom = { kind: "direct"; agent: Agent } | { kind: "group"; group: Group; members: Agent[] };

const roomIdOf = (room: ChatRoom): RoomId => (room.kind === "direct" ? room.agent.roomId : room.group.id);
const agentsOf = (room: ChatRoom): Agent[] => (room.kind === "direct" ? [room.agent] : room.members);

/** 右侧：一个房间的消息与输入框。 */
export function ChatView({ room, onAddMembers }: { room: ChatRoom; onAddMembers(): void }) {
  const roomId = roomIdOf(room);
  return (
    <main className="flex min-w-0 flex-1 flex-col">
      {room.kind === "direct" ? (
        <DirectHeader agent={room.agent} />
      ) : (
        <GroupHeader group={room.group} members={room.members} onAddMembers={onAddMembers} />
      )}
      {/* 换房间时重建消息列表与输入框：滚动位置与草稿属于各自的房间。 */}
      <MessageList key={roomId} room={room} />
      <Composer key={`composer-${roomId}`} room={room} />
    </main>
  );
}

function DirectHeader({ agent }: { agent: Agent }) {
  const status = statusView(agent.status);
  return (
    <header className="drag flex h-[52px] flex-none items-center gap-3 border-b border-line px-7 font-mono">
      <h1 className="truncate text-sm font-semibold">{agent.displayName}</h1>
      <span className="truncate text-xs text-muted">
        @{agent.handle} · {agent.model}
      </span>
      <StatusTag tone={status.tone} className="ml-auto flex-none">
        {status.label}
      </StatusTag>
    </header>
  );
}

function GroupHeader({ group, members, onAddMembers }: { group: Group; members: Agent[]; onAddMembers(): void }) {
  return (
    <header className="drag flex h-[52px] flex-none items-center gap-3 border-b border-line px-7 font-mono">
      <h1 className="flex-none truncate text-sm font-semibold">
        <span className="text-faint">#</span> {group.name}
      </h1>
      <div className="flex min-w-0 items-center gap-3 overflow-hidden">
        {members.map((agent) => (
          <StatusTag key={agent.id} tone={statusView(agent.status).tone} className="flex-none text-[11px]">
            <span className="text-text">{agent.displayName}</span>
          </StatusTag>
        ))}
      </div>
      <Button variant="ghost" size="sm" className="ml-auto flex-none" onClick={onAddMembers}>
        ＋ 成员
      </Button>
    </header>
  );
}

function MessageList({ room }: { room: ChatRoom }) {
  const roomId = roomIdOf(room);
  const agents = agentsOf(room);
  const { data: messages, error, isPending } = useMessages(roomId);
  const loadOlder = useLoadOlder(roomId);
  const scroller = useRef<HTMLDivElement>(null);
  const stickToBottom = useRef(true);
  /** 加载更早的消息前的内容高度：插入后据此保持视野里的消息不动。 */
  const heightBeforeOlder = useRef<number>(undefined);

  // 只在 Agent 状态真的变化时触发：`agents` 每次渲染都是新数组，用它作依赖时，
  // 点击“加载更早的消息”引起的重新渲染会在数据到达前就把记下的高度用掉。
  const statusKey = agents.map((agent) => `${agent.id}:${agent.status.state}`).join(",");

  // 新消息或状态变化后，原本停在底部就继续贴着底部；在前面插入更早的消息时，保持当前看到的位置。
  // biome-ignore lint/correctness/useExhaustiveDependencies: 两者是触发滚动的条件，回调里不需要读取它们。
  useLayoutEffect(() => {
    const element = scroller.current;
    if (!element) return;
    if (heightBeforeOlder.current !== undefined) {
      element.scrollTop += element.scrollHeight - heightBeforeOlder.current;
      heightBeforeOlder.current = undefined;
    } else if (stickToBottom.current) {
      element.scrollTop = element.scrollHeight;
    }
  }, [messages, statusKey]);

  if (isPending) return <div className="flex-1" />;
  if (error) return <p className="flex-1 px-7 py-6 text-sm text-danger">读取消息失败：{error.message}</p>;
  if (messages.length === 0 && agents.every((agent) => agent.status.state === "idle")) {
    return <EmptyChat room={room} />;
  }

  const showNames = room.kind === "group";
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
        {hasOlder(messages) && (
          <div className="mb-6 text-center">
            <Button
              variant="ghost"
              size="sm"
              disabled={loadOlder.isPending}
              onClick={() => {
                heightBeforeOlder.current = scroller.current?.scrollHeight;
                loadOlder.mutate(messages[0]?.seq ?? 1);
              }}
            >
              {loadOlder.isPending ? "加载中…" : "加载更早的消息"}
            </Button>
            {loadOlder.error && <p className="mt-1 text-xs text-danger">加载失败：{loadOlder.error.message}</p>}
          </div>
        )}
        {messages.map((message) => (
          <MessageItem key={message.id} message={message} now={now} />
        ))}
        {agents
          .filter((agent) => agent.status.state === "working")
          .map((agent) => (
            <div key={agent.id} className="mb-[26px]">
              <div className="mb-1 font-mono text-[13px] font-semibold text-accent">{agent.displayName}</div>
              <span className="font-mono text-[13px] text-muted">
                正在回复
                <span className="cursor-blink ml-1 inline-block h-[15px] w-[7px] bg-accent align-[-3px]" />
              </span>
            </div>
          ))}
        {agents.map((agent) =>
          agent.status.state === "error" ? (
            <div key={agent.id} className="mb-[26px] rounded-md border border-danger bg-danger-soft px-3.5 py-3">
              <div className="font-mono text-xs font-semibold text-danger">
                ✕ {showNames ? `${agent.displayName} 的` : ""}上一轮没有完成
              </div>
              <p className="selectable mt-1 text-sm leading-relaxed">{agent.status.reason}</p>
              <p className="text-xs text-muted">下一条消息到来时会自动重试。</p>
            </div>
          ) : null,
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
        {message.author.handle && <span className="text-faint">@{message.author.handle}</span>}
        <time className="text-faint" dateTime={message.createdAt}>
          {formatMessageTime(message.createdAt, now)}
        </time>
      </div>
      <Markdown>{message.body}</Markdown>
    </article>
  );
}

function EmptyChat({ room }: { room: ChatRoom }) {
  if (room.kind === "direct") {
    return (
      <div className="grid flex-1 place-items-center px-7">
        <div className="w-full max-w-[380px] text-center">
          <h2 className="font-mono text-[15px] font-semibold">和 {room.agent.displayName} 的对话从这里开始</h2>
          <div className="selectable mt-4 rounded-md border border-line px-3.5 py-3 text-left text-[13px] leading-relaxed whitespace-pre-wrap text-muted">
            <div className="mb-1 font-mono text-[11px] text-faint">人设</div>
            {room.agent.persona}
          </div>
        </div>
      </div>
    );
  }
  return (
    <div className="grid flex-1 place-items-center px-7">
      <div className="w-full max-w-[460px] text-center">
        <h2 className="font-mono text-[15px] font-semibold"># {room.group.name} 从这里开始</h2>
        <p className="mt-2 text-[13px] leading-relaxed text-muted">
          你的消息唤醒群里每个 agent；agent 的消息只唤醒它 @ 到的成员。
        </p>
        <ul className="mt-4 rounded-md border border-line px-3.5 py-2 text-left font-mono text-[13px]">
          {room.members.map((agent) => (
            <li key={agent.id} className="flex justify-between gap-3 py-1">
              <span className="truncate">{agent.displayName}</span>
              <span className="selectable text-muted">@{agent.handle}</span>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}

function Composer({ room }: { room: ChatRoom }) {
  const [draft, setDraft] = useState("");
  const send = useSendMessage(roomIdOf(room));
  const placeholder =
    room.kind === "direct" ? `给 ${room.agent.displayName} 发消息` : `发到 # ${room.group.name}，用 @handle 点名`;

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
