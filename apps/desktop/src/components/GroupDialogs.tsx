import {
  type DesktopAgent as Agent,
  type AgentId,
  type DesktopGroup as Group,
  ROOM_NAME_MAX,
  type RoomId,
} from "@crew/protocol";
import { useState } from "react";
import { activeMutes, muteUntilText } from "../lib/mutes";
import { groupsWithout, type NewGroupErrors, nonMembers, toggle, validateNewGroup } from "../lib/new-group";
import { useAddGroupMembers, useCreateGroup, useJoinGroups, useUnmuteAgent } from "../lib/queries";
import { Button } from "./ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogTitle } from "./ui/dialog";
import { Field, Input } from "./ui/input";

/** 新建群聊：名字与成员。 */
export function NewGroupDialog({
  open,
  onOpenChange,
  agents,
  onCreated,
}: {
  open: boolean;
  onOpenChange(open: boolean): void;
  agents: Agent[];
  onCreated(id: RoomId): void;
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        {/* 每次打开都是一张新表单。 */}
        {open && <NewGroupForm agents={agents} onCreated={onCreated} />}
      </DialogContent>
    </Dialog>
  );
}

function NewGroupForm({ agents, onCreated }: { agents: Agent[]; onCreated(id: RoomId): void }) {
  const create = useCreateGroup();
  const [name, setName] = useState("");
  const [agentIds, setAgentIds] = useState<AgentId[]>([]);
  const [errors, setErrors] = useState<NewGroupErrors>({});

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    const found = validateNewGroup({ name, agentIds });
    setErrors(found);
    if (Object.keys(found).length > 0) return;
    create.mutate({ name, agentIds }, { onSuccess: (group) => onCreated(group.id) });
  };

  return (
    <form onSubmit={submit} className="grid gap-3.5" noValidate>
      <DialogTitle>新建群聊</DialogTitle>
      <DialogDescription>你的消息会唤醒群里每个 agent；agent 之间用 @handle 点名。</DialogDescription>

      <Field label="名字" htmlFor="group-name" error={errors.name}>
        <Input
          id="group-name"
          autoFocus
          value={name}
          maxLength={ROOM_NAME_MAX}
          placeholder="例如 发版"
          aria-invalid={errors.name ? true : undefined}
          onChange={(event) => setName(event.target.value)}
        />
      </Field>

      <AgentPicker
        label="成员"
        agents={agents}
        selected={agentIds}
        onToggle={(id) => setAgentIds((ids) => toggle(ids, id))}
        error={errors.agentIds}
        empty="还没有 agent。先新建一个 agent 再建群聊。"
      />

      {create.error && <p className="text-xs text-danger">创建失败：{create.error.message}</p>}

      <div className="mt-1 flex justify-end gap-2">
        <DialogClose asChild>
          <Button>取消</Button>
        </DialogClose>
        <Button type="submit" variant="primary" disabled={agents.length === 0 || create.isPending}>
          创建
        </Button>
      </div>
    </form>
  );
}

/** 把 Agent 加进群聊。新成员只看到加入之后的消息。 */
export function AddMembersDialog({
  open,
  onOpenChange,
  group,
  agents,
  onAdded,
}: {
  open: boolean;
  onOpenChange(open: boolean): void;
  group: Group;
  agents: Agent[];
  onAdded(): void;
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>{open && <AddMembersForm group={group} agents={agents} onAdded={onAdded} />}</DialogContent>
    </Dialog>
  );
}

function AddMembersForm({ group, agents, onAdded }: { group: Group; agents: Agent[]; onAdded(): void }) {
  const add = useAddGroupMembers(group.id);
  const [agentIds, setAgentIds] = useState<AgentId[]>([]);
  const candidates = nonMembers(group, agents);

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (agentIds.length === 0) return;
    add.mutate(agentIds, { onSuccess: onAdded });
  };

  return (
    <form onSubmit={submit} className="grid gap-3.5" noValidate>
      <DialogTitle>添加成员到 # {group.name}</DialogTitle>
      <DialogDescription>新成员只看到加入之后的消息。</DialogDescription>

      <AgentPicker
        label="agent"
        agents={candidates}
        selected={agentIds}
        onToggle={(id) => setAgentIds((ids) => toggle(ids, id))}
        empty="所有 agent 都已经在群里了。"
      />

      {add.error && <p className="text-xs text-danger">添加失败：{add.error.message}</p>}

      <MutedMembers group={group} agents={agents} />

      <div className="mt-1 flex justify-end gap-2">
        <DialogClose asChild>
          <Button>取消</Button>
        </DialogClose>
        <Button type="submit" variant="primary" disabled={agentIds.length === 0 || add.isPending}>
          添加
        </Button>
      </div>
    </form>
  );
}

/** 静音了这个群的成员，可以在这里解除。没有静音的成员时不显示。 */
function MutedMembers({ group, agents }: { group: Group; agents: Agent[] }) {
  const unmute = useUnmuteAgent(group.id);
  const now = new Date();
  const muted = agents.flatMap((agent) => {
    const mute = activeMutes(group, now).get(agent.id);
    return mute ? [{ agent, mute }] : [];
  });
  if (muted.length === 0) return null;
  return (
    <div className="grid gap-1.5">
      <div className="font-mono text-[11px] text-muted">静音了这个群</div>
      <div className="rounded border border-line-strong">
        {muted.map(({ agent, mute }) => (
          <div
            key={agent.id}
            className="flex items-center gap-2.5 border-b border-line px-2.5 py-2 text-[13px] last:border-b-0"
          >
            <span className="truncate">{agent.displayName}</span>
            <span className="truncate font-mono text-[11px] text-muted">@{agent.handle}</span>
            <span className="ml-auto truncate text-[11.5px] text-faint">
              {mute.until === null ? "一直静音" : `静音到${muteUntilText(mute, now)}`}
            </span>
            <Button size="sm" disabled={unmute.isPending} onClick={() => unmute.mutate(agent.id)}>
              解除
            </Button>
          </div>
        ))}
      </div>
      {unmute.error && <p className="text-xs text-danger">解除失败：{unmute.error.message}</p>}
    </div>
  );
}

/** 勾选 Agent 的列表：名字、handle 与模型。 */
function AgentPicker({
  label,
  agents,
  selected,
  onToggle,
  error,
  empty,
}: {
  label: string;
  agents: Agent[];
  selected: AgentId[];
  onToggle(id: AgentId): void;
  error?: string;
  empty: string;
}) {
  return (
    <fieldset className="grid gap-1.5">
      <legend className="mb-1.5 font-mono text-[11px] text-muted">{label}</legend>
      {agents.length === 0 ? (
        <p className="text-xs text-muted">{empty}</p>
      ) : (
        <div
          className={
            error
              ? "max-h-56 overflow-y-auto rounded border border-danger"
              : "max-h-56 overflow-y-auto rounded border border-line-strong"
          }
        >
          {agents.map((agent) => (
            <label
              key={agent.id}
              className="flex cursor-pointer items-center gap-2.5 border-b border-line px-2.5 py-2 text-[13px] last:border-b-0 hover:bg-hover"
            >
              <input
                type="checkbox"
                className="accent-(--accent)"
                checked={selected.includes(agent.id)}
                onChange={() => onToggle(agent.id)}
              />
              <span className="truncate">{agent.displayName}</span>
              <span className="truncate font-mono text-[11px] text-muted">@{agent.handle}</span>
              <span className="ml-auto truncate font-mono text-[11px] text-faint">{agent.model}</span>
            </label>
          ))}
        </div>
      )}
      {error && <p className="text-xs text-danger">{error}</p>}
    </fieldset>
  );
}

/** 从私聊把这个 Agent 加进几个群聊。它只看到加入之后的消息。 */
export function JoinGroupsDialog({
  open,
  onOpenChange,
  agent,
  groups,
  onJoined,
}: {
  open: boolean;
  onOpenChange(open: boolean): void;
  agent: Agent;
  groups: Group[];
  onJoined(): void;
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>{open && <JoinGroupsForm agent={agent} groups={groups} onJoined={onJoined} />}</DialogContent>
    </Dialog>
  );
}

function JoinGroupsForm({ agent, groups, onJoined }: { agent: Agent; groups: Group[]; onJoined(): void }) {
  const join = useJoinGroups(agent.id);
  const [roomIds, setRoomIds] = useState<RoomId[]>([]);
  const candidates = groupsWithout(agent.id, groups);

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (roomIds.length === 0) return;
    join.mutate(roomIds, { onSuccess: onJoined });
  };

  return (
    <form onSubmit={submit} className="grid gap-3.5" noValidate>
      <DialogTitle>把 {agent.displayName} 加入群聊</DialogTitle>
      <DialogDescription>它只看到加入之后的消息。</DialogDescription>

      <fieldset className="grid gap-1.5">
        <legend className="mb-1.5 font-mono text-[11px] text-muted">群聊</legend>
        {candidates.length === 0 ? (
          <p className="text-xs text-muted">
            {groups.length === 0 ? "还没有群聊。" : `${agent.displayName} 已经在所有群聊里了。`}
          </p>
        ) : (
          <div className="max-h-56 overflow-y-auto rounded border border-line-strong">
            {candidates.map((group) => (
              <label
                key={group.id}
                className="flex cursor-pointer items-center gap-2.5 border-b border-line px-2.5 py-2 text-[13px] last:border-b-0 hover:bg-hover"
              >
                <input
                  type="checkbox"
                  className="accent-(--accent)"
                  checked={roomIds.includes(group.id)}
                  onChange={() => setRoomIds((ids) => toggle(ids, group.id))}
                />
                <span className="truncate"># {group.name}</span>
                <span className="ml-auto font-mono text-[11px] text-faint">{group.agentIds.length} 个 agent</span>
              </label>
            ))}
          </div>
        )}
      </fieldset>

      {join.error && <p className="text-xs text-danger">加入失败：{join.error.message}</p>}

      <div className="mt-1 flex justify-end gap-2">
        <DialogClose asChild>
          <Button>取消</Button>
        </DialogClose>
        <Button type="submit" variant="primary" disabled={roomIds.length === 0 || join.isPending}>
          加入
        </Button>
      </div>
    </form>
  );
}
