import { HANDLE_MAX, Handle } from "@crew/protocol";

export interface NewAgentInput {
  displayName: string;
  handle: string;
  persona: string;
  model: string;
}

export type NewAgentErrors = Partial<Record<keyof NewAgentInput, string>>;

/**
 * 下拉框当前的模型：用户选过且仍在列表里时用它，否则用列表的第一个；列表为空时为空字符串。
 * 模型列表从无到有时，Radix 的 Select 会用空字符串回调一次 onValueChange，空值同样回到第一个。
 */
export function selectedModel(chosen: string | undefined, models: readonly string[]): string {
  if (chosen && models.includes(chosen)) return chosen;
  return models[0] ?? "";
}

/**
 * 按名字建议 handle：转小写，英文字母与数字以外的字符换成 `-`。
 * 名字里没有英文字母或数字时（例如中文名）返回空字符串，由用户自己填。
 */
export function suggestHandle(displayName: string): string {
  return displayName
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+/, "")
    .slice(0, HANDLE_MAX)
    .replace(/-+$/, "");
}

/** 提交前检查必填项与 handle 的格式，文案与 Server 的校验一致。长度由输入框的 maxLength 限制。 */
export function validateNewAgent(input: NewAgentInput): NewAgentErrors {
  const errors: NewAgentErrors = {};
  if (!input.displayName.trim()) errors.displayName = "名字不能为空";
  if (!input.handle) errors.handle = "handle 不能为空";
  else {
    const handle = Handle.safeParse(input.handle);
    if (!handle.success) errors.handle = handle.error.issues[0]?.message;
  }
  if (!input.persona.trim()) errors.persona = "人设不能为空";
  if (!input.model) errors.model = "请选择模型";
  return errors;
}
