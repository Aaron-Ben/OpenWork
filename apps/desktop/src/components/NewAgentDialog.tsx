import type { AgentId } from "@crew/protocol";
import { useState } from "react";
import { DISPLAY_NAME_MAX, type NewAgentErrors, PERSONA_MAX, selectedModel, validateNewAgent } from "../lib/new-agent";
import { useCreateAgent, useModels } from "../lib/queries";
import { Button } from "./ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogTitle } from "./ui/dialog";
import { Field, Input, Textarea } from "./ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "./ui/select";

/** 新建 Agent：名字、人设与模型。模型列表来自 Computer 上报。 */
export function NewAgentDialog({
  open,
  onOpenChange,
  onCreated,
}: {
  open: boolean;
  onOpenChange(open: boolean): void;
  onCreated(id: AgentId): void;
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        {/* 每次打开都是一张新表单。 */}
        {open && <NewAgentForm onCreated={onCreated} />}
      </DialogContent>
    </Dialog>
  );
}

function NewAgentForm({ onCreated }: { onCreated(id: AgentId): void }) {
  const models = useModels(true);
  const create = useCreateAgent();
  const [displayName, setDisplayName] = useState("");
  const [persona, setPersona] = useState("");
  const [chosenModel, setChosenModel] = useState<string>();
  const [errors, setErrors] = useState<NewAgentErrors>({});

  const modelList = models.data ?? [];
  const model = selectedModel(chosenModel, modelList);
  const noModels = models.isSuccess && modelList.length === 0;

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    const input = { displayName, persona, model };
    const found = validateNewAgent(input);
    setErrors(found);
    if (Object.keys(found).length > 0) return;
    create.mutate(input, { onSuccess: (agent) => onCreated(agent.id) });
  };

  return (
    <form onSubmit={submit} className="grid gap-3.5" noValidate>
      <DialogTitle>新建 agent</DialogTitle>
      <DialogDescription>它在本机沙箱里运行，用你已登录的 OpenCode 账号调用模型。</DialogDescription>

      <Field label="名字" htmlFor="agent-name" error={errors.displayName}>
        <Input
          id="agent-name"
          autoFocus
          value={displayName}
          maxLength={DISPLAY_NAME_MAX}
          placeholder="例如 Alice"
          aria-invalid={errors.displayName ? true : undefined}
          onChange={(event) => setDisplayName(event.target.value)}
        />
      </Field>

      <Field label="人设" htmlFor="agent-persona" error={errors.persona}>
        <Textarea
          id="agent-persona"
          value={persona}
          maxLength={PERSONA_MAX}
          placeholder="它负责什么、怎样工作、说话风格"
          aria-invalid={errors.persona ? true : undefined}
          onChange={(event) => setPersona(event.target.value)}
        />
      </Field>

      <Field
        label="模型"
        htmlFor="agent-model"
        error={errors.model ?? (models.error ? `读取模型列表失败：${models.error.message}` : undefined)}
        hint={
          noModels ? "正在等 Computer 读取可用的模型。一直没有出现时，确认 OpenCode 已登录后重启 Crew。" : undefined
        }
      >
        <Select value={model} onValueChange={setChosenModel} disabled={modelList.length === 0}>
          <SelectTrigger id="agent-model" aria-invalid={errors.model ? true : undefined}>
            <SelectValue placeholder={models.isPending ? "读取中…" : "没有可用的模型"} />
          </SelectTrigger>
          <SelectContent>
            {modelList.map((name) => (
              <SelectItem key={name} value={name}>
                {name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </Field>

      {create.error && <p className="text-xs text-danger">创建失败：{create.error.message}</p>}

      <div className="mt-1 flex justify-end gap-2">
        <DialogClose asChild>
          <Button>取消</Button>
        </DialogClose>
        <Button type="submit" variant="primary" disabled={noModels || create.isPending}>
          创建
        </Button>
      </div>
    </form>
  );
}
