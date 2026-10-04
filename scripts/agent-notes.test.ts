import { describe, expect, it } from "vitest";
import { checkAgentNote } from "./agent-notes";

function note(status: string, body = "## 问题\n\n动机。\n\n## 考虑过的方案\n\n方案。\n") {
  return `# Agent Note: 标题\n\n${status}\n\n${body}`;
}

describe("checkAgentNote", () => {
  it("accepts a well-formed note in each lifecycle", () => {
    expect(checkAgentNote("proposed/architecture/2026-10-04-topic.md", note("Status: proposed"))).toEqual([]);
    expect(checkAgentNote("implemented/feature/2026-10-04-topic.md", note("Status: implemented"))).toEqual([]);
    expect(checkAgentNote("rejected/process/2026-10-04-topic.md", note("Status: rejected — 成本太高"))).toEqual([]);
    expect(checkAgentNote("legacy/architecture/2026-09-24-topic.md", note("Status: legacy"))).toEqual([]);
  });

  it("rejects a status that does not match the directory", () => {
    expect(checkAgentNote("implemented/feature/2026-10-04-topic.md", note("Status: proposed"))).toEqual([
      "implemented/feature/2026-10-04-topic.md：第 3 行的状态与所在目录 implemented/ 不一致",
    ]);
    expect(checkAgentNote("legacy/feature/2026-09-24-topic.md", note("Status: implemented"))).toEqual([
      "legacy/feature/2026-09-24-topic.md：第 3 行的状态与所在目录 legacy/ 不一致",
    ]);
  });

  it("rejects a rejected note without a reason", () => {
    expect(checkAgentNote("rejected/process/2026-10-04-topic.md", note("Status: rejected"))).toHaveLength(1);
  });

  it("rejects an unknown category and a malformed file name", () => {
    expect(checkAgentNote("proposed/misc/topic.md", note("Status: proposed"))).toEqual([
      "proposed/misc/topic.md：类别应为 feature、bug-fix、simplification、architecture、process、testing 之一",
      "proposed/misc/topic.md：文件名应为 yyyy-mm-dd-主题.md，主题用小写英文与短横线",
    ]);
  });

  it("rejects a file outside the lifecycle/category layout", () => {
    expect(checkAgentNote("archive/2026-10-04-topic.md", note("Status: proposed"))).toHaveLength(1);
  });

  it("rejects a body that does not start with 问题 or lacks 考虑过的方案", () => {
    const problems = checkAgentNote("proposed/feature/2026-10-04-topic.md", note("Status: proposed", "## 提议\n"));
    expect(problems).toEqual([
      "proposed/feature/2026-10-04-topic.md：正文应以 “## 问题” 开始",
      "proposed/feature/2026-10-04-topic.md：缺少 “## 考虑过的方案”",
    ]);
  });
});
