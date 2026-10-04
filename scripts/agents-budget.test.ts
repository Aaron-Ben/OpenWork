import { describe, expect, it } from "vitest";
import { checkAgentsBudget, countChars } from "./agents-budget";

describe("checkAgentsBudget", () => {
  it("does not count whitespace", () => {
    expect(countChars("# 标题\n\n- 一条 规则\n")).toBe(8);
  });

  it("accepts content at the limit", () => {
    expect(checkAgentsBudget("一二三", 3)).toEqual([]);
  });

  it("rejects content over the limit", () => {
    expect(checkAgentsBudget("一二三四", 3)).toEqual(["AGENTS.md：4 字，超出上限 3 字"]);
  });
});
