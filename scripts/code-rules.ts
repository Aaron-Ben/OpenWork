export interface CodeProblem {
  line: number;
  message: string;
}

/** `as unknown` 绕过类型检查，packages/AGENTS.md 禁止它。 */
const AS_UNKNOWN = /\bas\s+unknown\b/g;
/** 空的 catch：大括号里只有空白。写了注释的不算空。 */
const EMPTY_CATCH = /\bcatch\s*(?:\([^)]*\))?\s*\{\s*\}/g;

/**
 * 检查一个 TypeScript 或 JavaScript 文件是否违反 packages/AGENTS.md 中能机械检查的约定：
 * 禁止 `as unknown`；空的 catch 必须写明吞掉的错误与原因。
 */
export function checkCode(content: string): CodeProblem[] {
  const problems: CodeProblem[] = [];
  const lineOf = (index: number) => content.slice(0, index).split("\n").length;
  for (const match of content.matchAll(AS_UNKNOWN)) {
    problems.push({ line: lineOf(match.index), message: "禁止 `as unknown`：改为校验输入或修正类型" });
  }
  for (const match of content.matchAll(EMPTY_CATCH)) {
    problems.push({ line: lineOf(match.index), message: "空的 catch：写注释说明吞掉的是什么错误、为什么可以吞掉" });
  }
  return problems.sort((a, b) => a.line - b.line);
}
