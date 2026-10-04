import { describe, expect, it } from "vitest";
import { checkCode } from "./code-rules";

describe("checkCode", () => {
  it("accepts ordinary casts and catch blocks that explain what they swallow", () => {
    const code = [
      "const id = value as AgentId;",
      "try {",
      "  run();",
      "} catch {",
      "  // 文件不存在时没有可清理的内容。",
      "}",
    ].join("\n");
    expect(checkCode(code)).toEqual([]);
  });

  it("reports `as unknown` with its line", () => {
    expect(checkCode("const a = 1;\nconst b = a as unknown as string;")).toEqual([
      { line: 2, message: "禁止 `as unknown`：改为校验输入或修正类型" },
    ]);
  });

  it("reports an empty catch, with or without a binding and across lines", () => {
    const code = "try { a(); } catch {}\ntry { b(); } catch (error) {\n}";
    expect(checkCode(code).map((problem) => problem.line)).toEqual([1, 2]);
  });

  it("does not treat other empty blocks as catch blocks", () => {
    expect(checkCode("const noop = () => {};\nsetInterval(() => {}, 1000);")).toEqual([]);
  });
});
