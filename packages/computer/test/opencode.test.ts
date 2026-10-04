import { existsSync } from "node:fs";
import { chmod, mkdir, mkdtemp, readFile, realpath, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { AgentId, RuntimeSessionId } from "@crew/protocol";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { classifyFailure, derivedConfig, OpenCodeAdapter, readAuthContent, redact } from "../src/engine/opencode";
import type { TurnRequest } from "../src/engine/types";
import { type AgentLayout, confinementFor, prepareAgent, prepareRuntime } from "../src/home";

// 用假 opencode 在真实的 Seatbelt 下运行 Turn，不调用模型。

const repoRoot = fileURLToPath(new URL("../../../", import.meta.url));
const electron = join(repoRoot, "apps/desktop/node_modules/electron/dist/Electron.app/Contents/MacOS/Electron");
const fakeScript = fileURLToPath(new URL("./fixtures/fake-opencode.mjs", import.meta.url));

let root: string;
let dataHome: string;
let fakeOpencode: string;
let layout: AgentLayout;
let base: Omit<TurnRequest, "signal">;

beforeAll(async () => {
  root = await realpath(await mkdtemp(join(tmpdir(), "crew-opencode-test-")));
  dataHome = join(root, "user-data");
  await mkdir(join(dataHome, "opencode"), { recursive: true });
  await writeFile(join(dataHome, "opencode", "auth.json"), '{"deepseek":{"type":"api","key":"sk-test"}}');

  fakeOpencode = join(root, "opencode");
  await writeFile(
    fakeOpencode,
    `#!/bin/sh\nELECTRON_RUN_AS_NODE=1 exec '${await realpath(electron)}' '${fakeScript}' "$@"\n`,
  );
  await chmod(fakeOpencode, 0o755);

  const runtime = await prepareRuntime(join(root, "crew"), RuntimeSessionId.parse("session-1"));
  layout = await prepareAgent(runtime, {
    id: AgentId.parse("2f8c0b6e-3a1d-4c5e-9f7a-1b2c3d4e5f60"),
    displayName: "Alice",
    persona: "代码审查者",
  });
  base = {
    layout,
    confinement: await confinementFor(runtime, layout, [fakeOpencode]),
    model: "deepseek/deepseek-v4-pro",
    prompt: "帮我看看这个函数",
    sessionId: undefined,
    env: { CREW_SERVER_URL: "http://127.0.0.1:1234", FAKE_MODE: "ok" },
  };
});

afterAll(async () => {
  await rm(root, { recursive: true, force: true });
});

const adapter = () => new OpenCodeAdapter({ executable: fakeOpencode, userDataHome: dataHome });
const run = (overrides: Partial<TurnRequest> = {}) =>
  adapter().runTurn({ ...base, signal: new AbortController().signal, ...overrides });
const received = async () => JSON.parse(await readFile(join(layout.workDir, "received.json"), "utf8"));

describe("OpenCodeAdapter.runTurn", () => {
  it("runs opencode in the agent's work directory with the prompt on stdin and returns the session id", async () => {
    expect(await run()).toEqual({ ok: true, sessionId: "ses_fake_new" });
    const seen = await received();
    expect(seen.prompt).toBe("帮我看看这个函数");
    expect(seen.cwd).toBe(await realpath(layout.workDir));
    expect(seen.args).toEqual(
      expect.arrayContaining(["run", "--pure", "--format", "json", "--auto", "--model", "deepseek/deepseek-v4-pro"]),
    );
    expect(seen.args).not.toContain("--session");
  });

  it("isolates OpenCode's directories, passes credentials and config, and leaves database variables out", async () => {
    await run();
    const { env } = await received();
    expect(env).toMatchObject({
      HOME: layout.home,
      XDG_DATA_HOME: layout.engineDataDir,
      XDG_CONFIG_HOME: layout.configDir,
      XDG_CACHE_HOME: layout.cacheDir,
      XDG_STATE_HOME: layout.stateDir,
      OPENCODE_DISABLE_PROJECT_CONFIG: "1",
      OPENCODE_AUTH_CONTENT: '{"deepseek":{"type":"api","key":"sk-test"}}',
      CREW_SERVER_URL: "http://127.0.0.1:1234",
      DATABASE_URL: null,
    });
    expect(JSON.parse(env.OPENCODE_CONFIG_CONTENT)).toEqual({
      instructions: [layout.instructionsFile],
      permission: { "*": "allow" },
      provider: { deepseek: { models: { "deepseek-v4-pro": { status: "active" } } } },
    });
  });

  it("continues the given session", async () => {
    expect(await run({ sessionId: "ses_old" })).toEqual({ ok: true, sessionId: "ses_old" });
    expect((await received()).args).toEqual(expect.arrayContaining(["--session", "ses_old"]));
  });

  it("starts a new session once when the old one no longer exists", async () => {
    const outcome = await run({ sessionId: "ses_gone", env: { ...base.env, FAKE_MODE: "session-missing" } });
    expect(outcome).toEqual({ ok: true, sessionId: "ses_fake_new" });
  });

  it("reports a missing model with what to check", async () => {
    const outcome = await run({ env: { ...base.env, FAKE_MODE: "model-missing" } });
    expect(outcome.ok).toBe(false);
    if (outcome.ok) return;
    expect(outcome.error.kind).toBe("model-unavailable");
    expect(outcome.error.message).toContain("Model not found: nosuch/model.");
  });

  it("stops the whole process group, escalating to SIGKILL when SIGINT is ignored", async () => {
    const controller = new AbortController();
    const pending = adapter().runTurn({ ...base, signal: controller.signal, env: { ...base.env, FAKE_MODE: "hang" } });
    const pidFile = join(layout.workDir, "grandchild.pid");
    for (let i = 0; i < 200 && !existsSync(pidFile); i++) await new Promise((r) => setTimeout(r, 25));
    const grandchild = Number(await readFile(pidFile, "utf8"));

    controller.abort();
    expect(await pending).toEqual({ ok: false, error: { kind: "cancelled", message: "已停止" } });
    await new Promise((r) => setTimeout(r, 100));
    expect(() => process.kill(grandchild, 0)).toThrow();
  }, 15_000);

  it("does not start opencode when it was stopped before the turn began", async () => {
    const controller = new AbortController();
    controller.abort();
    const started = Date.now();
    const outcome = await adapter().runTurn({
      ...base,
      signal: controller.signal,
      env: { ...base.env, FAKE_MODE: "hang" },
    });
    expect(outcome).toEqual({ ok: false, error: { kind: "cancelled", message: "已停止" } });
    expect(Date.now() - started).toBeLessThan(2_000);
  }, 15_000);

  it("reports a login file it cannot read as a failed turn instead of throwing", async () => {
    const dir = join(root, "auth-is-a-directory");
    await mkdir(join(dir, "opencode", "auth.json"), { recursive: true });
    const outcome = await new OpenCodeAdapter({ executable: fakeOpencode, userDataHome: dir }).runTurn({
      ...base,
      signal: new AbortController().signal,
    });
    expect(outcome).toMatchObject({ ok: false, error: { kind: "unauthenticated" } });
  });

  it("stops an engine that floods its output", async () => {
    const outcome = await run({ env: { ...base.env, FAKE_MODE: "flood" } });
    expect(outcome).toMatchObject({ ok: false, error: { kind: "output-limit" } });
  }, 15_000);
});

describe("readAuthContent", () => {
  it("reports a missing login", async () => {
    expect(await readAuthContent(join(root, "nowhere"))).toMatchObject({ kind: "unauthenticated" });
  });

  it("rejects a login file that is not JSON instead of letting OpenCode fall back silently", async () => {
    const dir = join(root, "bad-auth");
    await mkdir(join(dir, "opencode"), { recursive: true });
    await writeFile(join(dir, "opencode", "auth.json"), "{ not json");
    expect(await readAuthContent(dir)).toMatchObject({ kind: "unauthenticated" });
  });

  it("rejects a login file over 64 KiB", async () => {
    const dir = join(root, "big-auth");
    await mkdir(join(dir, "opencode"), { recursive: true });
    await writeFile(join(dir, "opencode", "auth.json"), JSON.stringify({ k: "x".repeat(70 * 1024) }));
    expect(await readAuthContent(dir)).toMatchObject({ kind: "unauthenticated" });
  });
});

describe("classifyFailure", () => {
  it("recognizes a missing session, a missing model, rate limits and expired logins", () => {
    expect(classifyFailure(1, "Error: Session not found", undefined).kind).toBe("session-invalid");
    expect(classifyFailure(1, 'error="ProviderModelNotFoundError: Model not found: x/y."', "Unexpected").kind).toBe(
      "model-unavailable",
    );
    expect(classifyFailure(1, "APIError: 429 Too Many Requests", undefined).kind).toBe("rate-limited");
    expect(classifyFailure(1, "APIError: 401 Unauthorized", undefined).kind).toBe("unauthenticated");
  });

  it("recognizes sandbox-exec's own failures", () => {
    expect(classifyFailure(65, "sandbox-exec: invalid profile\n", undefined).kind).toBe("sandbox");
  });

  it("falls back to the reported error, then to the process failure", () => {
    expect(classifyFailure(1, "", "something broke")).toEqual({ kind: "reported", message: "something broke" });
    expect(classifyFailure(2, "boom\n", undefined)).toEqual({ kind: "process", message: "boom" });
  });
});

describe("derivedConfig and redact", () => {
  it("leaves out the provider entry for a model id without a provider", () => {
    expect(JSON.parse(derivedConfig("/a/AGENTS.md", "plain"))).toEqual({
      instructions: ["/a/AGENTS.md"],
      permission: { "*": "allow" },
    });
  });

  it("hides the agent directory and credentials in error text", () => {
    expect(redact("at /x/home/agent/work: Bearer abc token=def", "/x/home/agent")).toBe(
      "at <agent-home>/work: Bearer <redacted> token=<redacted>",
    );
  });
});
