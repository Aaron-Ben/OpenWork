import {
  type DesktopAgent as Agent,
  type AgentId,
  isSendBack,
  type RoomId,
  TASK_NOTE_MAX,
  type TaskStatus,
  type TaskView,
  type ThreadSummary,
  taskStatusLabel,
} from "@crew/protocol";
import { useMemo, useState } from "react";
import { cn } from "../lib/cn";
import { shouldSend, shouldSubmit } from "../lib/composer";
import { useTaskActions, useTasks, useThreads } from "../lib/queries";
import { BOARD_ORDER, FINISHED, groupByStatus, LIST_ORDER, nextStatuses } from "../lib/tasks";
import { AgentAvatar } from "./Avatar";
import { StatusIcon } from "./MessageParts";
import { column, ThreadView } from "./ThreadPanel";
import { Button } from "./ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogTitle } from "./ui/dialog";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "./ui/dropdown-menu";
import { Textarea } from "./ui/input";

export type TaskLayout = "list" | "board";

/**
 * 右栏里的任务。`selected` 为空时按列表或看板列出房间里的任务；否则显示这个任务的详情：
 * 状态与负责人的下拉菜单，群聊里下面是任务的讨论串。设计稿是 out/mockups/step5-threads-tasks.html。
 */
export function TaskPanel({
  roomId,
  kind,
  members,
  agents,
  layout,
  expanded,
  selected,
  onLayout,
  onSelect,
  onOpenRun,
}: {
  roomId: RoomId;
  kind: "direct" | "group";
  /** 房间里的 Agent 成员：负责人只能从他们中选。带着状态，正在讨论串里回复的显示实时活动。 */
  members: Agent[];
  agents: Agent[];
  layout: TaskLayout;
  expanded: boolean;
  selected: number | undefined;
  onLayout(layout: TaskLayout): void;
  onSelect(number: number | undefined): void;
  onOpenRun(runId: string): void;
}) {
  const tasks = useTasks(roomId);
  const threads = useThreads(roomId, kind === "group");
  const threadOf = useMemo(
    () => new Map((threads.data ?? []).map((thread) => [thread.parent.id, thread])),
    [threads.data],
  );
  const task = selected === undefined ? undefined : tasks.data?.find((candidate) => candidate.number === selected);

  if (selected !== undefined) {
    if (!task)
      return <p className="px-4 py-6 text-xs text-faint">{tasks.isPending ? "" : `任务 #${selected} 不存在`}</p>;
    return (
      <TaskDetail
        roomId={roomId}
        kind={kind}
        task={task}
        thread={threadOf.get(task.messageId)}
        members={members}
        agents={agents}
        expanded={expanded}
        onOpenRun={onOpenRun}
      />
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className={cn("flex items-center gap-2 px-4 pt-3 pb-1", column(expanded && layout === "list"))}>
        <div className="inline-flex rounded-lg bg-panel p-[3px] text-xs">
          {(["list", "board"] as const).map((option) => (
            <button
              key={option}
              type="button"
              onClick={() => onLayout(option)}
              className={cn(
                "rounded-md px-2.5 py-0.5",
                layout === option ? "bg-raised text-text shadow-card" : "text-muted hover:text-text",
              )}
            >
              {option === "list" ? "列表" : "看板"}
            </button>
          ))}
        </div>
      </div>
      <NewTask roomId={roomId} members={members} expanded={expanded && layout === "list"} />
      {tasks.error && <p className="px-4 py-2 text-xs text-danger">读取任务失败：{tasks.error.message}</p>}
      {tasks.data &&
        (layout === "board" ? (
          <Board tasks={tasks.data} threadOf={threadOf} onSelect={onSelect} />
        ) : (
          <List tasks={tasks.data} threadOf={threadOf} expanded={expanded} onSelect={onSelect} />
        ))}
    </div>
  );
}

/** 新建任务：一行标题与可选的负责人，回车创建。 */
function NewTask({ roomId, members, expanded }: { roomId: RoomId; members: Agent[]; expanded: boolean }) {
  const { create } = useTaskActions(roomId);
  const [title, setTitle] = useState("");
  const [assignee, setAssignee] = useState<AgentId | "">("");
  const submit = () => {
    if (!title.trim() || create.isPending) return;
    create.mutate(
      { title: title.trim(), assigneeId: assignee || undefined },
      {
        onSuccess: () => {
          setTitle("");
          setAssignee("");
        },
      },
    );
  };
  return (
    <div className={cn("px-4 pt-2", column(expanded))}>
      <div className="flex items-center gap-1.5 rounded-[10px] bg-panel px-2.5 py-1.5 focus-within:ring-2 focus-within:ring-accent-soft">
        <span className="text-faint">＋</span>
        <input
          value={title}
          onChange={(event) => {
            setTitle(event.target.value);
            if (create.error) create.reset();
          }}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.nativeEvent.isComposing) submit();
          }}
          placeholder="新建任务，回车创建"
          className="min-w-0 flex-1 bg-transparent py-0.5 text-[13px] outline-none placeholder:text-faint"
        />
        <select
          value={assignee}
          onChange={(event) => setAssignee(event.target.value as AgentId | "")}
          aria-label="负责人"
          className="max-w-[110px] rounded-md bg-transparent text-xs text-muted outline-none"
        >
          <option value="">未分配</option>
          {members.map((agent) => (
            <option key={agent.id} value={agent.id}>
              {agent.displayName}
            </option>
          ))}
        </select>
      </div>
      {create.error && <p className="mt-1 text-xs text-danger">新建失败：{create.error.message}</p>}
    </div>
  );
}

function List({
  tasks,
  threadOf,
  expanded,
  onSelect,
}: {
  tasks: TaskView[];
  threadOf: Map<string, ThreadSummary>;
  expanded: boolean;
  onSelect(number: number): void;
}) {
  const [showFinished, setShowFinished] = useState(false);
  const groups = groupByStatus(tasks);
  const finished = FINISHED.flatMap((status) => groups.get(status) ?? []);
  if (tasks.length === 0) {
    return (
      <p className={cn("px-5 py-6 text-[12.5px] leading-relaxed text-faint", column(expanded))}>
        还没有任务。在上面写一个标题，或者鼠标移到一条消息上点“转为任务”。
      </p>
    );
  }
  return (
    <div className="min-h-0 flex-1 overflow-y-auto px-3 pb-4">
      <div className={column(expanded)}>
        {LIST_ORDER.map((status) => {
          const group = groups.get(status) ?? [];
          if (group.length === 0) return null;
          return (
            <section key={status}>
              <GroupTitle status={status} count={group.length} />
              {group.map((task) => (
                <TaskRow key={task.id} task={task} thread={threadOf.get(task.messageId)} onSelect={onSelect} />
              ))}
            </section>
          );
        })}
        {finished.length > 0 && (
          <section>
            <button
              type="button"
              onClick={() => setShowFinished((open) => !open)}
              className="mt-3.5 mb-1.5 flex items-center gap-2 px-1 text-xs font-semibold text-faint hover:text-muted"
            >
              {showFinished ? "▾" : "▸"} 完成与关闭 <span className="font-mono font-normal">{finished.length}</span>
            </button>
            {showFinished &&
              finished.map((task) => (
                <TaskRow key={task.id} task={task} thread={threadOf.get(task.messageId)} onSelect={onSelect} />
              ))}
          </section>
        )}
      </div>
    </div>
  );
}

function GroupTitle({ status, count }: { status: TaskStatus; count: number }) {
  return (
    <h4 className="mt-3.5 mb-1.5 flex items-center gap-2 px-1 text-xs font-semibold text-muted">
      <StatusIcon status={status} />
      {taskStatusLabel(status)}
      <span className="font-mono font-normal text-faint">{count}</span>
    </h4>
  );
}

function TaskRow({
  task,
  thread,
  onSelect,
}: {
  task: TaskView;
  thread: ThreadSummary | undefined;
  onSelect(number: number): void;
}) {
  return (
    <button
      type="button"
      onClick={() => onSelect(task.number)}
      className="flex w-full items-center gap-2.5 rounded-[10px] px-2.5 py-2 text-left hover:bg-panel"
    >
      <span className="w-6 flex-none font-mono text-[11px] text-faint">#{task.number}</span>
      <span className="min-w-0 flex-1 truncate text-[13.5px]">{task.title}</span>
      <Replies thread={thread} />
      <Assignee task={task} />
    </button>
  );
}

function Replies({ thread }: { thread: ThreadSummary | undefined }) {
  if (!thread || thread.replies === 0) return null;
  return (
    <span className={cn("flex-none font-mono text-[11px]", thread.unread > 0 ? "text-accent" : "text-faint")}>
      {thread.unread > 0 ? `● ${thread.unread} 条新` : `${thread.replies} 条`}
    </span>
  );
}

function Assignee({ task }: { task: TaskView }) {
  return task.assignee ? (
    <AgentAvatar name={task.assignee.displayName} handle={task.assignee.handle} size={18} ring="var(--raised)" />
  ) : (
    <span className="flex-none text-[11px] text-faint">未分配</span>
  );
}

function Board({
  tasks,
  threadOf,
  onSelect,
}: {
  tasks: TaskView[];
  threadOf: Map<string, ThreadSummary>;
  onSelect(number: number): void;
}) {
  const groups = groupByStatus(tasks);
  return (
    <div className="grid min-h-0 flex-1 grid-cols-[repeat(3,minmax(128px,1fr))_minmax(112px,0.8fr)_minmax(104px,0.6fr)] gap-2.5 overflow-auto px-4 pt-2.5 pb-4">
      {BOARD_ORDER.map((status) => {
        const lane = groups.get(status) ?? [];
        return (
          <div
            key={status}
            className={cn(
              "flex min-h-0 flex-col gap-2 rounded-xl bg-panel p-2",
              FINISHED.includes(status) && "opacity-70",
            )}
          >
            <h4 className="flex items-center gap-2 px-1 pt-0.5 text-xs font-semibold text-muted">
              <StatusIcon status={status} />
              {taskStatusLabel(status)}
              <span className="ml-auto font-mono font-normal text-faint">{lane.length}</span>
            </h4>
            {lane.map((task) => (
              <button
                key={task.id}
                type="button"
                onClick={() => onSelect(task.number)}
                className="rounded-[10px] border border-line bg-raised px-2.5 py-2 text-left text-[13px] shadow-card hover:border-line-strong"
              >
                <span className="font-mono text-[11px] text-faint">#{task.number}</span>
                <span className="block leading-snug">{task.title}</span>
                <span className="mt-2 flex items-center gap-2">
                  <Replies thread={threadOf.get(task.messageId)} />
                  <span className="ml-auto">
                    <Assignee task={task} />
                  </span>
                </span>
              </button>
            ))}
          </div>
        );
      })}
    </div>
  );
}

/** 任务详情：状态与负责人的下拉菜单；群聊里下面是任务的讨论串，私聊里通知在时间线上。 */
function TaskDetail({
  roomId,
  kind,
  task,
  thread,
  members,
  agents,
  expanded,
  onOpenRun,
}: {
  roomId: RoomId;
  kind: "direct" | "group";
  task: TaskView;
  thread: ThreadSummary | undefined;
  members: Agent[];
  agents: Agent[];
  expanded: boolean;
  onOpenRun(runId: string): void;
}) {
  const handles = useMemo(() => new Set(agents.map((agent) => agent.handle)), [agents]);
  const header = <TaskProperties roomId={roomId} task={task} members={members} />;
  if (kind === "group" && thread) {
    return (
      <ThreadView
        key={task.id}
        groupId={roomId}
        members={members}
        handles={handles}
        parent={thread.parent}
        thread={thread}
        expanded={expanded}
        onOpenRun={onOpenRun}
        header={header}
      />
    );
  }
  return (
    <div className="min-h-0 flex-1 overflow-y-auto px-4 py-4">
      <div className={column(expanded)}>
        {header}
        {kind === "direct" && (
          <p className="mt-2 text-[12.5px] text-faint">私聊没有讨论串：这个任务的通知与讨论都在私聊的时间线上。</p>
        )}
      </div>
    </div>
  );
}

function TaskProperties({ roomId, task, members }: { roomId: RoomId; task: TaskView; members: Agent[] }) {
  const { setStatus, assign } = useTaskActions(roomId);
  const error = setStatus.error ?? assign.error;
  const finished = FINISHED.includes(task.status);
  /** 选了退回类的状态：先问要改什么，再改。 */
  const [sendBack, setSendBack] = useState<TaskStatus>();
  const choose = (status: TaskStatus) => {
    if (isSendBack(task.status, status)) {
      setStatus.reset();
      setSendBack(status);
    } else setStatus.mutate({ task, status });
  };
  return (
    <div className="mb-3.5">
      <div className="font-mono text-[11.5px] text-faint">#{task.number}</div>
      <h3 className="text-[17px] leading-snug font-semibold">{task.title}</h3>
      <div className="mt-3 grid grid-cols-[64px_1fr] items-center gap-x-2.5 gap-y-1.5 text-[12.5px]">
        <span className="text-faint">状态</span>
        <span>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <button type="button" className={pill}>
                <StatusIcon status={task.status} />
                {taskStatusLabel(task.status)}
                <span className="text-faint">⌄</span>
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start">
              {nextStatuses(task).map((status) => (
                <DropdownMenuItem key={status} className="font-sans" onSelect={() => choose(status)}>
                  <StatusIcon status={status} />
                  {taskStatusLabel(status)}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
        </span>
        <span className="text-faint">负责人</span>
        <span>
          <DropdownMenu>
            <DropdownMenuTrigger asChild disabled={finished}>
              <button type="button" className={cn(pill, finished && "opacity-60")}>
                {task.assignee ? (
                  <>
                    <AgentAvatar name={task.assignee.displayName} handle={task.assignee.handle} size={16} />
                    {task.assignee.displayName}
                  </>
                ) : (
                  <span className="text-muted">未分配</span>
                )}
                <span className="text-faint">⌄</span>
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start">
              {members.map((agent) => (
                <DropdownMenuItem
                  key={agent.id}
                  className="font-sans"
                  onSelect={() => assign.mutate({ task, agentId: agent.id })}
                >
                  <AgentAvatar name={agent.displayName} handle={agent.handle} size={16} />
                  {agent.displayName}
                </DropdownMenuItem>
              ))}
              {task.assignee && (
                <DropdownMenuItem
                  className="font-sans text-muted"
                  onSelect={() => assign.mutate({ task, agentId: null })}
                >
                  取消负责人
                </DropdownMenuItem>
              )}
            </DropdownMenuContent>
          </DropdownMenu>
        </span>
      </div>
      {error && !sendBack && <p className="mt-2 text-xs text-danger">{error.message}</p>}
      <Dialog
        open={sendBack !== undefined}
        onOpenChange={(open) => {
          if (!open) setSendBack(undefined);
        }}
      >
        <DialogContent>
          {sendBack && (
            <SendBackForm
              task={task}
              to={sendBack}
              pending={setStatus.isPending}
              error={setStatus.error?.message}
              onSubmit={(note) =>
                setStatus.mutate({ task, status: sendBack, note }, { onSuccess: () => setSendBack(undefined) })
              }
            />
          )}
        </DialogContent>
      </Dialog>
    </div>
  );
}

/** 退回任务时问一句要改什么。说明写进退回的通知，负责人被唤醒时就读到；留空也能退回。 */
function SendBackForm({
  task,
  to,
  pending,
  error,
  onSubmit,
}: {
  task: TaskView;
  to: TaskStatus;
  pending: boolean;
  error?: string;
  onSubmit(note: string | undefined): void;
}) {
  const [note, setNote] = useState("");
  const submit = () => onSubmit(note.trim() || undefined);
  return (
    <form
      className="grid gap-3.5"
      onSubmit={(event) => {
        event.preventDefault();
        if (!pending) submit();
      }}
    >
      <DialogTitle>
        退回 #{task.number}：{taskStatusLabel(task.status)} → {taskStatusLabel(to)}
      </DialogTitle>
      <DialogDescription>
        {task.assignee
          ? `告诉 ${task.assignee.displayName} 要改什么。说明写进退回的通知，它被唤醒时就能看到。`
          : "写一句为什么退回，会写进通知。"}
      </DialogDescription>
      <Textarea
        autoFocus
        value={note}
        maxLength={TASK_NOTE_MAX}
        placeholder="要改什么？（选填）"
        aria-label="退回的说明"
        onChange={(event) => setNote(event.target.value)}
        onKeyDown={(event) => {
          const key = { ...event, isComposing: event.nativeEvent.isComposing };
          // 请求进行中按 Enter 也不换行，只是不再提交。
          if (shouldSend(key)) event.preventDefault();
          if (shouldSubmit(key, pending)) submit();
        }}
      />
      {error && <p className="text-xs text-danger">{error}</p>}
      <div className="mt-1 flex justify-end gap-2">
        <DialogClose asChild>
          <Button>取消</Button>
        </DialogClose>
        <Button type="submit" variant="primary" disabled={pending}>
          退回
        </Button>
      </div>
    </form>
  );
}

const pill = "inline-flex items-center gap-1.5 rounded-lg bg-panel px-2.5 py-[3px] hover:bg-hover";
