import { readFileSync } from "node:fs";
import { checkCommitMessage } from "./commit-msg";

// lefthook 的 commit-msg 钩子：参数是 git 写好的提交说明文件。格式不对时以 1 退出，提交被拒绝。

const file = process.argv[2];
if (!file) {
  console.error("用法：tsx scripts/verify-commit-msg.ts <提交说明文件>");
  process.exit(1);
}
const problems = checkCommitMessage(readFileSync(file, "utf8"));
if (problems.length > 0) {
  console.error(`提交说明不符合格式：\n${problems.map((problem) => `- ${problem}`).join("\n")}`);
  console.error("格式见 .agents/skills/crew-commit/SKILL.md。");
  process.exit(1);
}
