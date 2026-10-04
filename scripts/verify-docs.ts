import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { relative, resolve } from "node:path";
import { checkAgentNote } from "./agent-notes";
import { checkAgentsBudget } from "./agents-budget";
import { findMissingPaths, referenceProjects } from "./doc-paths";
import { findBrokenLinks } from "./md-links";

// 文档检查：Markdown 相对链接、行内代码写到的路径、Agent Note 格式、根 AGENTS.md 字数。
// 由 `pnpm lint` 运行，有问题时以 1 退出。

const root = resolve(import.meta.dirname, "..");
const notesDir = resolve(root, ".agents/notes");

/**
 * 不检查行内代码路径的文档：已实现与已归档的 Note、旧版的子系统页记录的是当时的路径；
 * 任务文件写的是计划中、还没建的文件。
 */
const PATH_CHECK_EXEMPT = [
  ".agents/notes/implemented/",
  ".agents/notes/archived/",
  ".agents/tasks/",
  "docs/subsystems/collaboration.md",
  "docs/subsystems/collaboration-desktop.md",
];

/** 仓库中已跟踪与未跟踪（但未被忽略）的 Markdown 文件。已跟踪但在工作区中删除的文件不在其中。 */
function markdownFiles(): string[] {
  const output = execFileSync("git", ["ls-files", "--cached", "--others", "--exclude-standard", "--", "*.md"], {
    cwd: root,
    encoding: "utf8",
  });
  return output
    .split("\n")
    .filter(Boolean)
    .map((file) => resolve(root, file))
    .filter((file) => existsSync(file));
}

const agentsMd = readFileSync(resolve(root, "AGENTS.md"), "utf8");
const projects = referenceProjects(agentsMd);

const problems: string[] = [];
for (const file of markdownFiles()) {
  const content = readFileSync(file, "utf8");
  const path = relative(root, file);
  for (const { line, target } of findBrokenLinks(file, content)) {
    problems.push(`${path}:${line}：链接目标不存在 ${target}`);
  }
  if (!PATH_CHECK_EXEMPT.some((prefix) => path.startsWith(prefix))) {
    for (const { line, path: missing } of findMissingPaths(root, content, { projects })) {
      problems.push(`${path}:${line}：写到的路径不存在 ${missing}`);
    }
  }

  const notePath = relative(notesDir, file);
  if (!notePath.startsWith("..") && notePath !== "README.md") {
    problems.push(...checkAgentNote(notePath, content));
  }
}
problems.push(...checkAgentsBudget(agentsMd));

if (problems.length > 0) {
  console.error(problems.join("\n"));
  console.error(`\n文档检查发现 ${problems.length} 个问题。`);
  process.exit(1);
}
console.log("文档检查通过。");
