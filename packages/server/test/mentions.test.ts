import { describe, expect, it } from "vitest";
import { mentionedHandles } from "../src/mentions";

describe("mentionedHandles", () => {
  it("finds handles in order, lowercased and without duplicates", () => {
    expect(mentionedHandles("@Bob 和 @alice 看一下，@bob 先")).toEqual(["bob", "alice"]);
  });

  it("ignores code blocks, inline code, e-mail addresses and paths", () => {
    const body = [
      "```ts",
      "@decorator",
      "```",
      "`@inline` 不算，me@example.com 不算，/home/@user 不算",
      "~~~",
      "@tilde",
      "~~~",
      "（@carol）算",
    ].join("\n");
    expect(mentionedHandles(body)).toEqual(["carol"]);
  });

  it("drops a trailing hyphen and handles that are too long", () => {
    expect(mentionedHandles("@alice-请看")).toEqual(["alice"]);
    expect(mentionedHandles(`@${"a".repeat(33)}`)).toEqual([]);
  });

  it("treats an unclosed code fence as code until the end", () => {
    expect(mentionedHandles("@bob\n```\n@ghost")).toEqual(["bob"]);
  });
});
