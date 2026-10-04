/**
 * 根 AGENTS.md 的字数上限（不计空白字符）。
 *
 * 它每个会话都会进入上下文，所以控制长度。超出时先把内容移到负责的文档或精简，
 * 只有内容确实需要时才提高上限。
 */
export const AGENTS_MD_LIMIT = 2_800;

export function countChars(content: string): number {
  return content.replace(/\s/g, "").length;
}

/** @returns 超出上限时的问题描述；没有超出时为空数组。 */
export function checkAgentsBudget(content: string, limit = AGENTS_MD_LIMIT): string[] {
  const count = countChars(content);
  return count > limit ? [`AGENTS.md：${count} 字，超出上限 ${limit} 字`] : [];
}
