import { chmod, mkdir, realpath, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { request } from "../src/lib/server";
import { type BuiltApp, electronPath, startBuiltApp, until } from "./support/built-app";

// 冒烟测试：走真实入口。构建应用，启动构建好的 Server 与 Computer，只把模型换成假 opencode；
// 它在 Seatbelt 里经构建好的 `crew` 回复，断言回复落库。用 pnpm test:smoke 运行，pnpm check 包含它。

const fakeScript = fileURLToPath(new URL("./fixtures/fake-opencode.mjs", import.meta.url));

let app: BuiltApp;

describe.skipIf(process.platform !== "darwin")("built app", () => {
  beforeAll(async () => {
    // Computer 从 PATH 找 opencode，从 XDG_DATA_HOME 读登录文件。
    app = await startBuiltApp(async (root) => {
      const fakeBin = join(root, "bin");
      await mkdir(fakeBin);
      await writeFile(
        join(fakeBin, "opencode"),
        `#!/bin/sh\nELECTRON_RUN_AS_NODE=1 exec '${await realpath(electronPath)}' '${fakeScript}' "$@"\n`,
      );
      await chmod(join(fakeBin, "opencode"), 0o755);
      const dataHome = join(root, "data");
      await mkdir(join(dataHome, "opencode"), { recursive: true });
      await writeFile(join(dataHome, "opencode", "auth.json"), "{}");
      return { PATH: `${fakeBin}:${process.env.PATH}`, XDG_DATA_HOME: dataHome };
    });
  }, 120_000);

  afterAll(async () => {
    await app?.close();
  });

  it("delivers a user message to the agent and stores its reply sent through the shim in Seatbelt", async () => {
    const { client } = app;

    // Computer 启动后上报假 opencode 的模型列表。
    await until(async () => {
      const models = await request(client.desktop.models.$get());
      return models.includes("fake/model") ? models : undefined;
    }, 30_000);

    const agent = await request(
      client.desktop.agents.$post({ json: { displayName: "Smoke", persona: "冒烟测试", model: "fake/model" } }),
    );
    await request(
      client.desktop.rooms[":roomId"].messages.$post({ param: { roomId: agent.roomId }, json: { body: "ping" } }),
    );

    const reply = await until(async () => {
      const messages = await request(
        client.desktop.rooms[":roomId"].messages.$get({ param: { roomId: agent.roomId } }),
      );
      return messages.find((message) => message.author.kind === "agent");
    }, 30_000);
    expect(reply.author).toMatchObject({ kind: "agent", id: agent.id, displayName: "Smoke" });
    expect(reply.body).toBe("pong：`ls $HOME` 原样保留");

    // Turn 成功结束后，Agent 回到空闲。
    await until(async () => {
      const agents = await request(client.desktop.agents.$get());
      return agents.find((item) => item.id === agent.id && item.status.state === "idle");
    }, 30_000);
  }, 60_000);
});
