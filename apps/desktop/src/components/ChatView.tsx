import type { DesktopAgent as Agent, DesktopGroup as Group, RoomMessage as Message, RoomId } from "@crew/protocol";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { canSend, shouldSend } from "../lib/composer";
import { hasOlder, newestSeq } from "../lib/messages";
import { useLoadOlder, useMarkRead, useMessages, useRun, useSendMessage } from "../lib/queries";
import { formatDuration, liveView } from "../lib/runs";
import { isNearBottom } from "../lib/scroll";
import { statusIn, statusView } from "../lib/status";
import { formatMessageTime } from "../lib/time";
import { AgentAvatar, GroupAvatar, UserAvatar } from "./Avatar";
import { Markdown } from "./Markdown";
import { RunPanel } from "./RunPanel";
import { StatusTag } from "./StatusTag";
import { Button } from "./ui/button";

/** 打开的房间：与一个 Agent 的私聊，或一个群聊与其中的 Agent。 */
export type ChatRoom = { kind: "direct"; agent: Agent } | { kind: "group"; group: Group; members: Agent[] };

const roomIdOf = (room: ChatRoom): RoomId => (room.kind === "direct" ? room.agent.roomId : room.group.id);
const agentsOf = (room: ChatRoom): Agent[] => (room.kind === "direct" ? [room.agent] : room.members);

/** 把房间里 Agent 的状态换成它们在这个房间的状态：在别的房间回复时，这里显示空闲。 */
function inRoom(room: ChatRoom): ChatRoom {
  const roomId = roomIdOf(room);
  const scope = (agent: Agent): Agent => ({ ...agent, status: statusIn(agent.status, roomId) });
  return room.kind === "direct" ? { ...room, agent: scope(room.agent) } : { ...room, members: room.members.map(scope) };
}

/**
 * 右侧：一个房间的消息与输入框。`agents` 是全部 Agent：消息的作者可能已经不在群里，
 * 头像与 @handle 的高亮都按全部 Agent 查找。
 */
export function ChatView({
  room,
  agents,
  onAddMembers,
  onJoinGroups,
}: {
  room: ChatRoom;
  agents: Agent[];
  onAddMembers(): void;
  onJoinGroups(): void;
}) {
  const roomId = roomIdOf(room);
  room = inRoom(room);
  // 面板开着时换房间，面板跟着换成新房间的记录；选中的一轮只属于原来的房间。
  const [panelOpen, setPanelOpen] = useState(false);
  const [selected, setSelected] = useState<{ roomId: RoomId; runId: string }>();
  const selectedRun = selected?.roomId === roomId ? selected.runId : undefined;
  const openRun = (runId: string) => {
    setSelected({ roomId, runId });
    setPanelOpen(true);
  };
  const runsButton = (
    <Button
      variant={panelOpen ? "secondary" : "ghost"}
      size="sm"
      className="flex-none"
      onClick={() => setPanelOpen((open) => !open)}
    >
      ◷ 运行记录
    </Button>
  );

  return (
    <>
      <main className="flex min-w-0 flex-1 flex-col">
        {room.kind === "direct" ? (
          <DirectHeader agent={room.agent} onJoinGroups={onJoinGroups} runsButton={runsButton} />
        ) : (
          <GroupHeader group={room.group} members={room.members} onAddMembers={onAddMembers} runsButton={runsButton} />
        )}
        {/* 换房间时重建消息列表与输入框：滚动位置与草稿属于各自的房间。 */}
        <MessageList key={roomId} room={room} agents={agents} onOpenRun={openRun} />
        <Composer key={`composer-${roomId}`} room={room} />
      </main>
      {panelOpen && (
        <RunPanel
          title={room.kind === "direct" ? `${room.agent.displayName} 的运行记录` : `# ${room.group.name} 的运行记录`}
          scope={room.kind === "direct" ? { agentId: room.agent.id } : { roomId }}
          agents={agents}
          members={room.kind === "group" ? room.members : []}
          selectedId={selectedRun}
          onSelect={(runId) => setSelected(runId ? { roomId, runId } : undefined)}
          onClose={() => setPanelOpen(false)}
        />
      )}
    </>
  );
}

function DirectHeader({
  agent,
  onJoinGroups,
  runsButton,
}: {
  agent: Agent;
  onJoinGroups(): void;
  runsButton: React.ReactNode;
}) {
  const status = statusView(agent.status);
  return (
    <header className="drag flex h-[52px] flex-none items-center gap-3 border-b border-line px-6 font-mono">
      <AgentAvatar name={agent.displayName} handle={agent.handle} size={28} />
      <h1 className="truncate font-sans text-sm font-semibold">{agent.displayName}</h1>
      <span className="truncate text-xs text-muted">
        @{agent.handle} · {agent.model}
      </span>
      <StatusTag tone={status.tone} className="ml-auto flex-none">
        {status.label}
      </StatusTag>
      {runsButton}
      <Button variant="ghost" size="sm" className="flex-none" onClick={onJoinGroups}>
        加入群聊…
      </Button>
    </header>
  );
}

function GroupHeader({
  group,
  members,
  onAddMembers,
  runsButton,
}: {
  group: Group;
  members: Agent[];
  onAddMembers(): void;
  runsButton: React.ReactNode;
}) {
  return (
    <header className="drag flex h-[52px] flex-none items-center gap-3 border-b border-line px-6 font-mono">
      <GroupAvatar members={members.map((agent) => ({ name: agent.displayName, handle: agent.handle }))} size={28} />
      <h1 className="flex-none truncate font-sans text-sm font-semibold">
        <span className="text-faint">#</span> {group.name}
      </h1>
      <span className="flex-none text-xs text-muted">{members.length} 个 agent</span>
      <div className="flex min-w-0 items-center gap-1.5 overflow-hidden">
        {members.map((agent) => (
          <span
            key={agent.id}
            className="flex flex-none items-center gap-1.5 rounded-full border border-line bg-raised py-[2px] pr-2 pl-[3px] text-[11.5px] text-muted"
          >
            <AgentAvatar name={agent.displayName} handle={agent.handle} size={18} />
            {agent.displayName}
            {agent.status.state !== "idle" && (
              <StatusTag tone={statusView(agent.status).tone} className="text-[11px]">
                {statusView(agent.status).label}
              </StatusTag>
            )}
          </span>
        ))}
      </div>
      <span className="ml-auto" />
      {runsButton}
      <Button variant="ghost" size="sm" className="flex-none" onClick={onAddMembers}>
        ＋ 成员
      </Button>
    </header>
  );
}

function MessageList({
  room,
  agents: allAgents,
  onOpenRun,
}: {
  room: ChatRoom;
  agents: Agent[];
  onOpenRun(runId: string): void;
}) {
  const roomId = roomIdOf(room);
  const agents = agentsOf(room);
  const { data: messages, error, isPending } = useMessages(roomId);
  const handles = useMemo(() => new Set(allAgents.map((agent) => agent.handle)), [allAgents]);
  useMarkReadWhileOpen(roomId, messages);
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
          <MessageItem key={message.id} message={message} now={now} handles={handles} onOpenRun={onOpenRun} />
        ))}
        {agents.map((agent) =>
          agent.status.state === "working" ? (
            <LiveActivity key={agent.id} agent={agent} runId={agent.status.runId} onOpenRun={onOpenRun} />
          ) : null,
        )}
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

function MessageItem({
  message,
  now,
  handles,
  onOpenRun,
}: {
  message: Message;
  now: Date;
  handles: ReadonlySet<string>;
  onOpenRun(runId: string): void;
}) {
  const { author, runId } = message;
  return (
    <article className="mb-[22px] flex gap-3">
      {author.kind === "user" ? (
        <UserAvatar />
      ) : (
        <AgentAvatar name={author.displayName} handle={author.handle ?? author.id} ring="var(--bg)" />
      )}
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
            {message.heldBefore > 0 && (
              <span className="text-warn">
                ↻ 看到新消息后改写了回复{message.heldBefore > 1 ? `（被拦下 ${message.heldBefore} 次）` : ""}
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
      </div>
    </article>
  );
}

/** Agent 正在这个房间跑一轮：实时列出最近几次工具调用与“思考中”，点击打开这一轮的记录。 */
function LiveActivity({ agent, runId, onOpenRun }: { agent: Agent; runId: string; onOpenRun(runId: string): void }) {
  const { data: run } = useRun(runId);
  const now = useNow(1000);
  const view = run ? liveView(run) : { recent: [], thinking: true, steps: 0 };
  const elapsed = run ? formatDuration(now - new Date(run.startedAt).getTime()) : "";
  return (
    <div className="mb-[18px] flex gap-3">
      <AgentAvatar name={agent.displayName} handle={agent.handle} size={22} className="mt-2 ml-[7px]" />
      <button
        type="button"
        onClick={() => onOpenRun(runId)}
        className="w-full max-w-[520px] rounded-md border border-l-2 border-line border-l-accent bg-raised px-3 py-2 text-left font-mono"
      >
        <span className="flex items-center gap-2 text-[12.5px] text-muted">
          {agent.displayName} 正在回复
          <span className="cursor-blink inline-block h-[13px] w-[7px] bg-accent" />
          {run && (
            <span className="ml-auto text-[11px] text-faint">
              已运行 {elapsed}
              {view.steps > 0 ? ` · 第 ${view.steps} 步` : ""}
            </span>
          )}
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

/**
 * 房间开着、窗口在前台时，把用户的已读位置推进到最新一条：打开房间时一次，之后每来新消息、
 * 窗口回到前台时再推进。窗口在后台时到达的消息保持未读，侧栏显示未读数。
 */
function useMarkReadWhileOpen(roomId: RoomId, messages: readonly Message[] | undefined) {
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
