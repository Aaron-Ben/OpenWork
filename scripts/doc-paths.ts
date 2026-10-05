import { existsSync } from "node:fs";
import { resolve } from "node:path";

export interface MissingPath {
  line: number;
  path: string;
}

export interface PathCheckOptions {
  /** 参考项目名（小写）到它在本机的根目录。路径写成 `raft:packages/...` 时到那里检查。 */
  projects?: Record<string, string>;
  exists?: (path: string) => boolean;
}

/** 本仓库的顶层目录。以它们开头的行内代码视为仓库路径。 */
const REPO_DIRS = new Set(["apps", "packages", "docs", "scripts", ".agents", ".claude"]);
const INLINE_CODE = /`([^`]+)`/g;
/** 引用其他项目的路径写成 `项目:路径`，例如 `raft:packages/cli/src/main.ts`。 */
const OTHER_PROJECT = /^([a-z]+):(.+)$/i;
/** 含这些字符的是模式或占位符，例如 `test/*.test.ts`、`runtime/<session>/`。 */
const PATTERN = /[*<>{}$\s]/;
/** 根 AGENTS.md 参考项目表的一行：`| Raft | \`/路径\` | 参考什么 |`。 */
const PROJECT_ROW = /^\|\s*([A-Za-z]+)\s*\|\s*`([^`]+)`\s*\|/;

/** 从根 AGENTS.md 的参考项目表读出项目名与本机路径，作为 `projects` 选项。 */
export function referenceProjects(agentsMd: string): Record<string, string> {
  const projects: Record<string, string> = {};
  for (const line of agentsMd.split("\n")) {
    const match = line.match(PROJECT_ROW);
    if (match?.[1] && match[2]) projects[match[1].toLowerCase()] = match[2];
  }
  return projects;
}

/**
 * 找出 Markdown 行内代码中写到、但不存在的路径。
 *
 * - 以本仓库顶层目录开头的路径，相对仓库根目录检查。
 * - 写成 `项目:路径` 的路径，在该项目的本机目录里检查；项目不在 `projects` 中或目录不存在时跳过。
 * - 忽略围栏代码块、模式与占位符。末尾的 `:行号` 不参与检查。
 *
 * @param root 仓库根目录的绝对路径。
 */
export function findMissingPaths(root: string, content: string, options: PathCheckOptions = {}): MissingPath[] {
  const exists = options.exists ?? existsSync;
  const projects = options.projects ?? {};
  const missing: MissingPath[] = [];
  let fence: string | undefined;
  content.split("\n").forEach((line, index) => {
    const marker = line.match(/^\s*(```|~~~)/)?.[1];
    if (marker) {
      fence = fence === marker ? undefined : (fence ?? marker);
      return;
    }
    if (fence) return;
    for (const match of line.matchAll(INLINE_CODE)) {
      const code = match[1] ?? "";
      if (PATTERN.test(code)) continue;
      const target = locate(root, code.replace(/:\d+(?:-\d+)?$/, ""), projects, exists);
      if (target && !exists(target)) missing.push({ line: index + 1, path: code });
    }
  });
  return missing;
}

/** 行内代码对应的待检查路径；不是需要检查的路径时返回 undefined。 */
function locate(
  root: string,
  code: string,
  projects: Record<string, string>,
  exists: (path: string) => boolean,
): string | undefined {
  const other = code.match(OTHER_PROJECT);
  if (other?.[1] && other[2]) {
    const projectRoot = projects[other[1].toLowerCase()];
    return projectRoot && exists(projectRoot) ? resolve(projectRoot, other[2]) : undefined;
  }
  const first = code.split("/")[0] ?? "";
  return code.includes("/") && REPO_DIRS.has(first) ? resolve(root, code) : undefined;
}
