/** 提交类型。与 .agents/skills/crew-commit/SKILL.md 一致。 */
const TYPES = ["feat", "fix", "refactor", "docs", "test", "chore"];
const SUBJECT = new RegExp(`^(?:${TYPES.join("|")})(?:\\([a-z0-9-]+\\))?: (.+)$`);
const CJK = /[一-鿿]/;
/** git 自己生成的说明，不要求格式。 */
const GENERATED = /^(?:Merge |Revert |fixup! |squash! |amend! )/;

/**
 * 检查提交说明：标题是 `<type>(<scope>): <中文描述>`，有正文时标题后空一行。
 * 以 `#` 开头的行是 git 的注释，不参与检查。
 *
 * @returns 问题描述；符合格式时为空数组。
 */
export function checkCommitMessage(message: string): string[] {
  const lines = message.split("\n").filter((line) => !line.startsWith("#"));
  while (lines.length > 0 && lines[lines.length - 1]?.trim() === "") lines.pop();
  const subject = lines[0] ?? "";
  if (GENERATED.test(subject)) return [];

  const problems: string[] = [];
  const match = subject.match(SUBJECT);
  if (!match) {
    problems.push(`标题要写成 <type>(<scope>): <描述>，type 是 ${TYPES.join("、")} 之一，scope 可省略`);
  } else if (!CJK.test(match[1] ?? "")) {
    problems.push("标题的描述用中文写");
  }
  if (lines.length > 1 && lines[1]?.trim() !== "") {
    problems.push("标题与正文之间空一行");
  }
  return problems;
}
