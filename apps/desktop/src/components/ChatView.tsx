import type {
  DesktopAgent as Agent,
  DesktopGroup as Group,
  MessageView,
  RoomId,
  TaskView,
  ThreadSummary,
} from "@crew/protocol";
import { useLayoutEffect, useMemo, useRef, useState } from "react";
import { cn } from "../lib/cn";
import { hasOlder } from "../lib/messages";
import { useLoadOlder, useMessages, useSendMessage, useTaskActions, useTasks, useThreads } from "../lib/queries";
import { isNearBottom } from "../lib/scroll";
import { statusIn, statusView } from "../lib/status";
import { openCount } from "../lib/tasks";
import { AgentAvatar, GroupAvatar } from "./Avatar";
import { Composer, LiveActivity, MessageItem, useMarkReadWhileOpen } from "./MessageParts";
import { RunPanel } from "./RunPanel";
import { SidebarToggle, SidePanel } from "./SidePanel";
import { StatusTag } from "./StatusTag";
import { type TaskLayout, TaskPanel } from "./TaskPanel";
import { ThreadPanel } from "./ThreadPanel";
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

/** 右栏显示什么：运行记录、讨论串（列表或其中一个）或任务（列表、看板或其中一个）。 */
type PanelView = "runs" | "threads" | "tasks";

/** 右栏里选中的东西只属于打开它时的房间：换房间后右栏保持开着，显示新房间的列表。 */
type Selection = { roomId: RoomId; runId?: string; parent?: MessageView; task?: number };

/**
 * 聊天：一个房间的消息与输入框，以及右栏（运行记录、讨论串或任务）。`agents` 是全部 Agent：消息的作者可能已经不在群里，
 * 头像与 @handle 的高亮都按全部 Agent 查找。右栏放大时占去聊天区，聊天区让出来。侧栏收起时（`sidebarHidden`），
 * 顶栏最左边是“显示侧栏”。
 */
export function ChatView({
  room,
  agents,
  sidebarHidden,
  onShowSidebar,
  onAddMembers,
  onJoinGroups,
}: {
  room: ChatRoom;
  agents: Agent[];
  sidebarHidden: boolean;
  onShowSidebar(): void;
  onAddMembers(): void;
  onJoinGroups(): void;
}) {
  const roomId = roomIdOf(room);
  const scoped = inRoom(room);
  const [view, setView] = useState<PanelView>();
  const [expanded, setExpanded] = useState(false);
  const [layout, setLayout] = useState<TaskLayout>("list");
  const [selection, setSelection] = useState<Selection>();
  /** 消息往上滚过了顶栏：顶栏下沿显示一道渐隐，代替分隔线。 */
  const [scrolled, setScrolled] = useState(false);
  const selected = selection?.roomId === roomId ? selection : undefined;
  // 私聊没有讨论串：从群聊换到私聊时，讨论串的右栏关上。
  const panel = room.kind === "direct" && view === "threads" ? undefined : view;
  const threads = useThreads(roomId, room.kind === "group");
  const tasks = useTasks(roomId);
  const { convert } = useTaskActions(roomId);

  const resize = (next: boolean) => {
    setExpanded(next);
    // 看板在不放大的右栏里放不下五列：还原时切回列表。
    if (!next) setLayout("list");
  };
  const close = () => {
    setView(undefined);
    resize(false);
  };
  const open = (next: PanelView, picked?: Omit<Selection, "roomId">) => {
    setView(next);
    if (picked) setSelection({ roomId, ...picked });
  };
  const toggle = (next: PanelView) => {
    if (panel === next) close();
    else open(next, {});
  };
  const openRun = (runId: string) => open("runs", { runId });
  const openThread = (parent: MessageView) => open("threads", { parent });
  const openTask = (task: TaskView) => open("tasks", { task: task.number });
  const convertMessage = (message: MessageView) => convert.mutate(message.id, { onSuccess: (task) => openTask(task) });

  const unreadThreads = (threads.data ?? []).filter((thread) => thread.unread > 0).length;
  const tools = (
    <div className="flex flex-none gap-0.5 rounded-[10px] bg-panel p-[3px] font-sans">
      {room.kind === "group" && (
        <Tool active={panel === "threads"} onClick={() => toggle("threads")}>
          讨论串
          <span className={cn("font-mono text-[11px]", unreadThreads > 0 ? "text-accent" : "text-faint")}>
            {threads.data?.length ?? ""}
          </span>
        </Tool>
      )}
      <Tool active={panel === "tasks"} onClick={() => toggle("tasks")}>
        任务
        <span className="font-mono text-[11px] text-faint">{tasks.data ? openCount(tasks.data) : ""}</span>
      </Tool>
      <Tool active={panel === "runs"} onClick={() => toggle("runs")}>
        运行记录
      </Tool>
    </div>
  );

  const groupName = room.kind === "group" ? `# ${room.group.name}` : room.agent.displayName;
  const parent = panel === "threads" ? selected?.parent : undefined;
  const taskNumber = panel === "tasks" ? selected?.task : undefined;
  const openTaskView = taskNumber === undefined ? undefined : tasks.data?.find((task) => task.number === taskNumber);
  // 右栏正打开着的那条消息：讨论串的宿主消息，或任务的宿主消息。
  const openMessageId = parent?.id ?? openTaskView?.messageId;
  const showSidebar = sidebarHidden ? <SidebarToggle hidden onClick={onShowSidebar} /> : undefined;

  const title =
    panel === "runs"
      ? "运行记录"
      : panel === "threads"
        ? parent
          ? "讨论串"
          : "全部讨论串"
        : openTaskView
          ? `#${openTaskView.number} ${openTaskView.title}`
          : "任务";
  const subtitle =
    panel === "runs"
      ? room.kind === "group"
        ? `${groupName} · 包括讨论串里的轮次`
        : `${groupName} 的全部轮次`
      : panel === "threads"
        ? parent
          ? `${groupName} · ${parent.author.kind === "user" ? "你" : parent.author.displayName}的消息`
          : `${groupName} · ${threads.data?.length ?? 0} 个`
        : taskNumber !== undefined
          ? `${groupName} · 任务`
          : `${groupName} · ${tasks.data ? openCount(tasks.data) : 0} 个未完成`;
  const back =
    parent || taskNumber !== undefined ? () => setSelection({ roomId, parent: undefined, task: undefined }) : undefined;

  return (
    <>
      <main className={cn("min-w-0 flex-1 flex-col", panel && expanded ? "hidden" : "flex")}>
        {scoped.kind === "direct" ? (
          <DirectHeader
            agent={scoped.agent}
            leading={showSidebar}
            fade={scrolled}
            narrow={panel !== undefined}
            onJoinGroups={onJoinGroups}
            tools={tools}
          />
        ) : (
          <GroupHeader
            group={scoped.group}
            members={scoped.members}
            leading={showSidebar}
            fade={scrolled}
            narrow={panel !== undefined}
            onAddMembers={onAddMembers}
            tools={tools}
          />
        )}
        {/* 换房间时重建消息列表与输入框：滚动位置与草稿属于各自的房间。 */}
        <MessageList
          key={roomId}
          room={scoped}
          agents={agents}
          threads={threads.data}
          tasks={tasks.data}
          openMessageId={openMessageId}
          onOpenRun={openRun}
          onOpenThread={room.kind === "group" ? openThread : undefined}
          onOpenTask={openTask}
          onConvert={convertMessage}
          onScrolled={setScrolled}
          hidden={panel !== undefined && expanded}
        />
        {convert.error && (
          <p className="max-w-[736px] px-7 pb-1 text-xs text-danger">转为任务失败：{convert.error.message}</p>
        )}
        <RoomComposer key={`composer-${roomId}`} room={scoped} />
      </main>
      {panel && (
        <SidePanel
          title={title}
          subtitle={subtitle}
          expanded={expanded}
          tools={tools}
          leading={showSidebar}
          onExpandedChange={resize}
          onBack={back}
          onClose={close}
        >
          {panel === "runs" ? (
            <RunPanel
              scope={room.kind === "direct" ? { agentId: room.agent.id } : { roomId }}
              agents={agents}
              members={room.kind === "group" ? room.members : []}
              selectedId={selected?.runId}
              expanded={expanded}
              onSelect={(runId) => setSelection({ roomId, runId })}
            />
          ) : panel === "threads" ? (
            <ThreadPanel
              groupId={roomId}
              members={room.kind === "group" ? room.members : []}
              agents={agents}
              parent={parent}
              expanded={expanded}
              onOpen={openThread}
              onOpenRun={openRun}
            />
          ) : (
            <TaskPanel
              roomId={roomId}
              kind={room.kind}
              members={agentsOf(room)}
              agents={agents}
              layout={layout}
              expanded={expanded}
              selected={taskNumber}
              onLayout={(next) => {
                setLayout(next);
                // 看板在不放大的右栏里放不下五列：选看板时右栏自动放大。
                if (next === "board") setExpanded(true);
              }}
              onSelect={(number) => setSelection({ roomId, task: number })}
              onOpenRun={openRun}
            />
          )}
        </SidePanel>
      )}
    </>
  );
}

function Tool({ active, onClick, children }: { active: boolean; onClick(): void; children: React.ReactNode }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "inline-flex h-[26px] items-center gap-1.5 rounded-[7px] px-2.5 text-xs whitespace-nowrap",
        active ? "bg-raised text-text shadow-card" : "text-muted hover:text-text",
      )}
    >
      {children}
    </button>
  );
}

/**
 * 聊天顶栏与消息区同一个底色，没有分隔线；消息往上滚过顶部时，顶栏下沿出现一道渐隐。
 * 设计稿是 out/mockups/step5-cards.html 的方案 A。
 */
function headerClass(fade: boolean): string {
  return cn(
    // 横向裁掉放不下的内容，纵向不裁：下沿的渐隐画在顶栏外面。
    "drag relative z-10 flex h-[52px] flex-none items-center gap-3 overflow-x-clip bg-bg font-mono",
    fade &&
      "after:pointer-events-none after:absolute after:inset-x-0 after:top-full after:h-[18px] after:bg-linear-to-b after:from-bg after:to-transparent",
  );
}

function DirectHeader({
  agent,
  leading,
  fade,
  narrow,
  onJoinGroups,
  tools,
}: {
  agent: Agent;
  /** 侧栏收起时的“显示侧栏”。 */
  leading: React.ReactNode;
  fade: boolean;
  /** 右栏打开时聊天变窄：顶栏只留头像、名字、状态与视图切换。 */
  narrow: boolean;
  onJoinGroups(): void;
  tools: React.ReactNode;
}) {
  const status = statusView(agent.status);
  return (
    <header className={cn(headerClass(fade), leading ? "pr-6 pl-[84px]" : "px-6")}>
      {leading}
      <AgentAvatar name={agent.displayName} handle={agent.handle} size={28} />
      <h1 className="truncate font-sans text-sm font-semibold">{agent.displayName}</h1>
      {!narrow && (
        <span className="truncate text-xs text-muted">
          @{agent.handle} · {agent.model}
        </span>
      )}
      <StatusTag tone={status.tone} className="ml-auto flex-none">
        {status.label}
      </StatusTag>
      {tools}
      {!narrow && (
        <Button variant="ghost" size="sm" className="flex-none" onClick={onJoinGroups}>
          加入群聊…
        </Button>
      )}
    </header>
  );
}

function GroupHeader({
  group,
  members,
  leading,
  fade,
  narrow,
  onAddMembers,
  tools,
}: {
  group: Group;
  members: Agent[];
  leading: React.ReactNode;
  fade: boolean;
  /** 右栏打开时聊天变窄：顶栏只留头像、名字与视图切换。 */
  narrow: boolean;
  onAddMembers(): void;
  tools: React.ReactNode;
}) {
  return (
    <header className={cn(headerClass(fade), leading ? "pr-6 pl-[84px]" : "px-6")}>
      {leading}
      <GroupAvatar members={members.map((agent) => ({ name: agent.displayName, handle: agent.handle }))} size={28} />
      <h1 className={cn("truncate font-sans text-sm font-semibold", !narrow && "flex-none")}>
        <span className="text-faint">#</span> {group.name}
      </h1>
      {!narrow && <span className="flex-none text-xs text-muted">{members.length} 个 agent</span>}
      <div className={cn("min-w-0 items-center gap-1.5 overflow-hidden", narrow ? "hidden" : "flex")}>
        {members.map((agent) => (
          <span
            key={agent.id}
            className="flex flex-none items-center gap-1.5 rounded-full bg-panel py-[2px] pr-2 pl-[3px] text-[11.5px] text-muted"
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
      {tools}
      {!narrow && (
        <Button variant="ghost" size="sm" className="flex-none" onClick={onAddMembers}>
          ＋ 成员
        </Button>
      )}
    </header>
  );
}

function MessageList({
  room,
  agents: allAgents,
  threads,
  tasks,
  openMessageId,
  onOpenRun,
  onOpenThread,
  onOpenTask,
  onConvert,
  onScrolled,
  hidden,
}: {
  room: ChatRoom;
  agents: Agent[];
  threads: ThreadSummary[] | undefined;
  tasks: TaskView[] | undefined;
  /** 右栏正打开着的那条消息：讨论串或任务的宿主消息。 */
  openMessageId: string | undefined;
  onOpenRun(runId: string): void;
  /** 群聊里才有：打开一条消息的讨论串。 */
  onOpenThread?(message: MessageView): void;
  onOpenTask(task: TaskView): void;
  onConvert(message: MessageView): void;
  /** 消息区是否已经往上滚过了顶部。 */
  onScrolled(scrolled: boolean): void;
  /** 右栏放大时聊天区让出来：看不见的消息不标为已读，重新出现时接着贴底。 */
  hidden: boolean;
}) {
  const roomId = roomIdOf(room);
  const agents = agentsOf(room);
  const { data: messages, error, isPending } = useMessages(roomId);
  const handles = useMemo(() => new Set(allAgents.map((agent) => agent.handle)), [allAgents]);
  const threadOf = useMemo(() => new Map((threads ?? []).map((thread) => [thread.parent.id, thread])), [threads]);
  const taskOf = useMemo(() => new Map<string, TaskView>((tasks ?? []).map((task) => [task.messageId, task])), [tasks]);
  useMarkReadWhileOpen(roomId, hidden ? undefined : messages);
  const loadOlder = useLoadOlder(roomId);
  const scroller = useRef<HTMLDivElement>(null);
  const stickToBottom = useRef(true);
  /** 加载更早的消息前的内容高度：插入后据此保持视野里的消息不动。 */
  const heightBeforeOlder = useRef<number>(undefined);

  // 只在 Agent 状态真的变化时触发：`agents` 每次渲染都是新数组，用它作依赖时，
  // 点击“加载更早的消息”引起的重新渲染会在数据到达前就把记下的高度用掉。
  const statusKey = agents.map((agent) => `${agent.id}:${agent.status.state}`).join(",");

  // 新消息、状态变化或重新出现后，原本停在底部就继续贴着底部；在前面插入更早的消息时，保持当前看到的位置。
  // 隐藏期间（display: none）设置滚动位置不起作用，所以重新出现时还要再贴一次。
  // biome-ignore lint/correctness/useExhaustiveDependencies: 三者是触发滚动的条件，回调里不需要读取它们。
  useLayoutEffect(() => {
    const element = scroller.current;
    if (!element) return;
    if (heightBeforeOlder.current !== undefined) {
      element.scrollTop += element.scrollHeight - heightBeforeOlder.current;
      heightBeforeOlder.current = undefined;
    } else if (stickToBottom.current) {
      element.scrollTop = element.scrollHeight;
    }
  }, [messages, statusKey, hidden]);

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
        onScrolled(event.currentTarget.scrollTop > 0);
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
          <MessageItem
            key={message.id}
            message={message}
            now={now}
            handles={handles}
            onOpenRun={onOpenRun}
            thread={threadOf.get(message.id)}
            onOpenThread={onOpenThread}
            task={taskOf.get(message.id)}
            onOpenTask={onOpenTask}
            onConvert={onConvert}
            highlighted={message.id === openMessageId}
          />
        ))}
        {agents.map((agent) =>
          agent.status.state === "working" ? (
            <LiveActivity
              key={agent.id}
              agent={agent}
              runId={agent.status.runId}
              roomId={roomId}
              onOpenRun={onOpenRun}
            />
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

function RoomComposer({ room }: { room: ChatRoom }) {
  const send = useSendMessage(roomIdOf(room));
  const placeholder =
    room.kind === "direct" ? `给 ${room.agent.displayName} 发消息` : `发到 # ${room.group.name}，用 @handle 点名`;
  return (
    <Composer
      placeholder={placeholder}
      className="max-w-[736px] px-7 pb-[18px]"
      send={{
        mutate: (body, options) => send.mutate({ body }, options),
        isPending: send.isPending,
        error: send.error,
        reset: send.reset,
      }}
    />
  );
}
