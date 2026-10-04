import { describe, expect, it } from "vitest";
import { encodeMessage, readMessage, ServerBootstrap } from "../src";

async function* linesOf(...lines: string[]) {
  yield* lines;
}

const bootstrap = {
  runtimeSessionId: "session-1",
  desktopToken: "desktop",
  computerToken: "computer",
};

describe("readMessage", () => {
  it("parses the first line and ignores the rest", async () => {
    const message = await readMessage(linesOf(JSON.stringify(bootstrap), "ignored"), ServerBootstrap);
    expect(message).toEqual(bootstrap);
  });

  it("round-trips a message written by encodeMessage", async () => {
    const line = encodeMessage(bootstrap).trimEnd();
    expect(await readMessage(linesOf(line), ServerBootstrap)).toEqual(bootstrap);
  });

  it("rejects input that ends before the first line", async () => {
    await expect(readMessage(linesOf(), ServerBootstrap)).rejects.toThrow("输入在第一行之前结束");
  });

  it("rejects a first line that does not match the schema", async () => {
    const line = JSON.stringify({ ...bootstrap, desktopToken: "" });
    await expect(readMessage(linesOf(line), ServerBootstrap)).rejects.toThrow();
  });
});
