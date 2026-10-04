import { afterAll, beforeAll, describe, expect, it } from "vitest";
import {
  createTestApp,
  TEST_COMPUTER_TOKEN,
  TEST_DESKTOP_TOKEN,
  TEST_RENDERER_ORIGIN,
  type TestApp,
} from "./support/app";

// 凭证分区与 CORS。业务接口见 api.test.ts。

let t: TestApp;
beforeAll(async () => {
  t = await createTestApp();
});
afterAll(async () => {
  await t.close();
});

function bearer(token: string) {
  return { Authorization: `Bearer ${token}` };
}

describe("desktop routes", () => {
  it("reject a request without a token with a JSON error", async () => {
    const response = await t.app.request("/desktop/models");
    expect(response.status).toBe(401);
    expect(await response.json()).toEqual({ error: "凭证无效" });
  });

  it("reject the computer token", async () => {
    const response = await t.app.request("/desktop/models", { headers: bearer(TEST_COMPUTER_TOKEN) });
    expect(response.status).toBe(401);
  });

  it("reject a token of a different length", async () => {
    const response = await t.app.request("/desktop/models", { headers: bearer(`${TEST_DESKTOP_TOKEN}x`) });
    expect(response.status).toBe(401);
  });
});

describe("computer routes", () => {
  it("reject the desktop token", async () => {
    const response = await t.app.request("/computer/connect", { method: "POST", headers: bearer(TEST_DESKTOP_TOKEN) });
    expect(response.status).toBe(401);
  });

  it("accept the computer token", async () => {
    const response = await t.app.request("/computer/connect", { method: "POST", headers: bearer(TEST_COMPUTER_TOKEN) });
    expect(response.status).toBe(204);
  });
});

describe("agent routes", () => {
  it("reject the desktop and computer tokens", async () => {
    for (const token of [TEST_DESKTOP_TOKEN, TEST_COMPUTER_TOKEN]) {
      const response = await t.app.request("/agent/reply", { method: "POST", headers: bearer(token) });
      expect(response.status).toBe(401);
    }
  });
});

describe("CORS", () => {
  it("answer a preflight from the renderer origin without a token", async () => {
    const response = await t.app.request("/desktop/models", {
      method: "OPTIONS",
      headers: {
        Origin: TEST_RENDERER_ORIGIN,
        "Access-Control-Request-Method": "GET",
        "Access-Control-Request-Headers": "authorization",
      },
    });
    expect(response.status).toBe(204);
    expect(response.headers.get("Access-Control-Allow-Origin")).toBe(TEST_RENDERER_ORIGIN);
  });

  it("allow the renderer origin to read responses", async () => {
    const response = await t.app.request("/desktop/models", {
      headers: { ...bearer(TEST_DESKTOP_TOKEN), Origin: TEST_RENDERER_ORIGIN },
    });
    expect(response.headers.get("Access-Control-Allow-Origin")).toBe(TEST_RENDERER_ORIGIN);
  });

  it("give no CORS header to another origin", async () => {
    const response = await t.app.request("/desktop/models", {
      headers: { ...bearer(TEST_DESKTOP_TOKEN), Origin: "http://localhost:3000" },
    });
    expect(response.headers.get("Access-Control-Allow-Origin")).toBeNull();
  });
});
