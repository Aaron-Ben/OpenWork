import { describe, expect, it } from "vitest";
import { findBrokenLinks } from "./md-links";

const existing = new Set(["/repo/docs/architecture.md", "/repo/AGENTS.md", "/repo/docs/a b.md"]);
const exists = (path: string) => existing.has(path);
const check = (content: string) => findBrokenLinks("/repo/docs/README.md", content, exists);

describe("findBrokenLinks", () => {
  it("accepts links to existing files, relative to the document", () => {
    expect(check("[架构](architecture.md) 与 [规则](../AGENTS.md)")).toEqual([]);
  });

  it("reports a link to a missing file with its line number", () => {
    expect(check("第一行\n见 [测试](testing.md)")).toEqual([{ line: 2, target: "testing.md" }]);
  });

  it("ignores anchors, line suffixes and percent-encoding when locating the file", () => {
    expect(check("[a](architecture.md#1-依赖方向) [b](architecture.md:42) [c](a%20b.md)")).toEqual([]);
  });

  it("ignores external links and pure anchors", () => {
    expect(check("[x](https://example.com/missing.md) [y](mailto:a@b.c) [z](#问题)")).toEqual([]);
  });

  it("ignores links inside fenced code blocks and inline code", () => {
    expect(check("```markdown\n[示例](missing.md)\n```\n写法是 `[文字](missing.md)`")).toEqual([]);
  });
});
