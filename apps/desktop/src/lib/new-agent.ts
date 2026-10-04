export interface NewAgentInput {
  displayName: string;
  persona: string;
  model: string;
}

export type NewAgentErrors = Partial<Record<keyof NewAgentInput, string>>;

/** 名字与人设最多多少字符，与 Server 的校验一致；输入框用它限制长度。 */
export const DISPLAY_NAME_MAX = 40;
export const PERSONA_MAX = 4_000;

/**
 * 下拉框当前的模型：用户选过且仍在列表里时用它，否则用列表的第一个；列表为空时为空字符串。
 * 模型列表从无到有时，Radix 的 Select 会用空字符串回调一次 onValueChange，空值同样回到第一个。
 */
export function selectedModel(chosen: string | undefined, models: readonly string[]): string {
  if (chosen && models.includes(chosen)) return chosen;
  return models[0] ?? "";
}

/** 提交前检查必填项，文案与 Server 的校验一致。长度由输入框的 maxLength 限制。 */
export function validateNewAgent(input: NewAgentInput): NewAgentErrors {
  const errors: NewAgentErrors = {};
  if (!input.displayName.trim()) errors.displayName = "名字不能为空";
  if (!input.persona.trim()) errors.persona = "人设不能为空";
  if (!input.model) errors.model = "请选择模型";
  return errors;
}
