// Agent Note 的位置与格式，规则见 .agents/notes/README.md。

/** `legacy` 是 Rust 版的决策记录，只作参考，见 .agents/notes/README.md。 */
const LIFECYCLES = ["proposed", "implemented", "rejected", "legacy"] as const;
const CATEGORIES = ["feature", "bug-fix", "simplification", "architecture", "process", "testing"];
const FILE_NAME = /^\d{4}-\d{2}-\d{2}-[a-z0-9]+(?:-[a-z0-9]+)*\.md$/;

function expectedStatus(lifecycle: (typeof LIFECYCLES)[number], line: string): boolean {
  switch (lifecycle) {
    case "proposed":
      return line === "Status: proposed";
    case "implemented":
      return line === "Status: implemented";
    case "rejected":
      return /^Status: rejected — \S/.test(line);
    case "legacy":
      return line === "Status: legacy";
  }
}

/**
 * 检查一份 Agent Note 的路径与格式。
 *
 * @param path 相对 `.agents/notes/` 的路径，例如 `proposed/architecture/2026-10-04-topic.md`。
 * @returns 每个问题一条描述；没有问题时为空数组。
 */
export function checkAgentNote(path: string, content: string): string[] {
  const problems: string[] = [];
  const [lifecycle, category, fileName, ...rest] = path.split("/");

  if (!LIFECYCLES.includes(lifecycle as (typeof LIFECYCLES)[number]) || !fileName || rest.length > 0) {
    return [`${path}：路径应为 {${LIFECYCLES.join("|")}}/{类别}/yyyy-mm-dd-主题.md`];
  }
  if (!category || !CATEGORIES.includes(category)) {
    problems.push(`${path}：类别应为 ${CATEGORIES.join("、")} 之一`);
  }
  if (!FILE_NAME.test(fileName)) {
    problems.push(`${path}：文件名应为 yyyy-mm-dd-主题.md，主题用小写英文与短横线`);
  }

  const lines = content.split("\n");
  if (!lines[0]?.startsWith("# Agent Note: ")) {
    problems.push(`${path}：第 1 行应以 “# Agent Note: ” 开头`);
  }
  if (lines[1] !== "") {
    problems.push(`${path}：第 2 行应为空行`);
  }
  if (!expectedStatus(lifecycle as (typeof LIFECYCLES)[number], lines[2] ?? "")) {
    problems.push(`${path}：第 3 行的状态与所在目录 ${lifecycle}/ 不一致`);
  }

  const headings = lines.filter((line) => line.startsWith("## "));
  if (headings[0] !== "## 问题") {
    problems.push(`${path}：正文应以 “## 问题” 开始`);
  }
  if (!headings.includes("## 考虑过的方案")) {
    problems.push(`${path}：缺少 “## 考虑过的方案”`);
  }
  return problems;
}
