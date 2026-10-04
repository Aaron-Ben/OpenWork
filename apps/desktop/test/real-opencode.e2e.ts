import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { request } from "../src/lib/server";
import { type BuiltApp, startBuiltApp, until } from "./support/built-app";

// 真实模型的 e2e：与冒烟测试相同的构建产物与启动路径，但用本机已登录的 OpenCode 调用真实模型。
// 不进 pnpm check，用 CREW_E2E_MODEL=<模型> pnpm test:e2e 手动运行。
// 没有指定模型、没有 opencode 或没有登录时整组跳过：模型由运行的人选，避免误用昂贵或不可用的模型。

const model = process.env.CREW_E2E_MODEL;

function opencodeReady(): boolean {
  if (process.platform !== "darwin" || !model) return false;
  try {
    execFileSync("/usr/bin/which", ["opencode"], { stdio: "ignore" });
  } catch {
    // which 找不到时以非零状态退出：本机没有安装 opencode。
    return false;
  }
  const dataHome = process.env.XDG_DATA_HOME || join(homedir(), ".local", "share");
  return existsSync(join(dataHome, "opencode", "auth.json"));
}

let app: BuiltApp;

describe.skipIf(!opencodeReady())("built app with the real OpenCode", () => {
  beforeAll(async () => {
    app = await startBuiltApp(() => ({}));
  }, 120_000);

  afterAll(async () => {
    await app?.close();
  });

  it("gets a reply from a real model through the shim", async () => {
    const { client } = app;
    if (!model) throw new Error("没有指定 CREW_E2E_MODEL"); // skipIf 已经保证不会走到这里
    // 先等 Computer 上报模型，确认指定的模型在本机 OpenCode 中可用。
    const models = await until(async () => {
      const list = await request(client.desktop.models.$get());
      return list.length > 0 ? list : undefined;
    }, 60_000);
    expect(models, `本机 OpenCode 列出的模型里没有 ${model}`).toContain(model);

    const agent = await request(
      client.desktop.agents.$post({
        json: { displayName: "E2E", persona: "你在参加一次自动化测试，照指令回复。", model },
      }),
    );
    await request(
      client.desktop.rooms[":roomId"].messages.$post({
        param: { roomId: agent.roomId },
        json: { body: "这是一条自动化测试消息。请回复一条消息，正文只写 pong。" },
      }),
    );

    const reply = await until(async () => {
      const messages = await request(
        client.desktop.rooms[":roomId"].messages.$get({ param: { roomId: agent.roomId } }),
      );
      return messages.find((message) => message.author.kind === "agent");
    }, 240_000);
    expect(reply.author).toMatchObject({ kind: "agent", id: agent.id });
    expect(reply.body.toLowerCase(), `模型的回复：${reply.body}`).toContain("pong");
  }, 300_000);
});
