import { describe, expect, it } from "vitest";
import { findMissingPaths, referenceProjects } from "./doc-paths";

const existing = new Set([
  "/repo/apps/desktop/electron/main.ts",
  "/repo/docs/testing.md",
  "/repo/packages/server/src",
  "/code/raft",
  "/code/raft/packages/cli/src/main.ts",
]);
const exists = (path: string) => existing.has(path);
const projects = { raft: "/code/raft", dsh: "/code/dsh" };
const check = (content: string) => findMissingPaths("/repo", content, { exists, projects });

describe("findMissingPaths", () => {
  it("accepts repository paths that exist, with or without a line suffix or trailing slash", () => {
    expect(check("见 `apps/desktop/electron/main.ts:42` 与 `docs/testing.md`，以及 `packages/server/src/`")).toEqual(
      [],
    );
  });

  it("reports a repository path that no longer exists, with its line number", () => {
    expect(check("第一行\n导航在 `apps/desktop/src/main/navigation.ts`")).toEqual([
      { line: 2, path: "apps/desktop/src/main/navigation.ts" },
    ]);
  });

  it("checks a reference project's path in that project's checkout", () => {
    expect(check("`raft:packages/cli/src/main.ts` 与 `raft:packages/cli/src/gone.ts`")).toEqual([
      { line: 1, path: "raft:packages/cli/src/gone.ts" },
    ]);
  });

  it("skips projects that are unknown or not checked out on this machine", () => {
    expect(check("`opencode:packages/opencode/src/session/prompt.ts` 与 `dsh:docs/missing.md`")).toEqual([]);
  });

  it("ignores patterns, placeholders, commands and words that are not repository paths", () => {
    expect(
      check("`packages/*/test/` `runtime/<session>/bin/crew` `pnpm --filter x test` `~/.crew` `out/main` `crew reply`"),
    ).toEqual([]);
  });

  it("ignores fenced code blocks", () => {
    expect(check("```bash\ncat docs/missing.md\n`docs/missing.md`\n```")).toEqual([]);
  });
});

describe("referenceProjects", () => {
  it("reads project names and local paths from the AGENTS.md table", () => {
    const table =
      "| 名称 | 路径 | 参考什么 |\n|---|---|---|\n| Raft | `/code/raft` | runtime |\n| DSH | `/code/dsh` | 文档 |";
    expect(referenceProjects(table)).toEqual({ raft: "/code/raft", dsh: "/code/dsh" });
  });
});
