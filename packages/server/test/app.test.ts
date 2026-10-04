import { describe, expect, it } from "vitest";
import { createApp } from "../src/app";

const desktopToken = "desktop-token-0123456789";
const computerToken = "computer-token-0123456789";
const rendererOrigin = "http://localhost:5173";

function newApp() {
  return createApp({ desktopToken, computerToken, rendererOrigin });
}

function bearer(token: string) {
  return { Authorization: `Bearer ${token}` };
}

describe("desktop routes", () => {
  it("reject a request without a token", async () => {
    const response = await newApp().request("/desktop/status");
    expect(response.status).toBe(401);
  });

  it("reject the computer token", async () => {
    const response = await newApp().request("/desktop/status", { headers: bearer(computerToken) });
    expect(response.status).toBe(401);
  });

  it("reject a token of a different length", async () => {
    const response = await newApp().request("/desktop/status", { headers: bearer(`${desktopToken}x`) });
    expect(response.status).toBe(401);
  });

  it("report that the computer has not connected yet", async () => {
    const response = await newApp().request("/desktop/status", { headers: bearer(desktopToken) });
    expect(response.status).toBe(200);
    expect(await response.json()).toEqual({ computerConnected: false });
  });
});

describe("computer routes", () => {
  it("reject the desktop token", async () => {
    const response = await newApp().request("/computer/connect", {
      method: "POST",
      headers: bearer(desktopToken),
    });
    expect(response.status).toBe(401);
  });

  it("mark the computer as connected for later status requests", async () => {
    const app = newApp();
    const connect = await app.request("/computer/connect", { method: "POST", headers: bearer(computerToken) });
    expect(connect.status).toBe(204);

    const status = await app.request("/desktop/status", { headers: bearer(desktopToken) });
    expect(await status.json()).toEqual({ computerConnected: true });
  });
});

describe("CORS", () => {
  it("answer a preflight from the renderer origin without a token", async () => {
    const response = await newApp().request("/desktop/status", {
      method: "OPTIONS",
      headers: {
        Origin: rendererOrigin,
        "Access-Control-Request-Method": "GET",
        "Access-Control-Request-Headers": "authorization",
      },
    });
    expect(response.status).toBe(204);
    expect(response.headers.get("Access-Control-Allow-Origin")).toBe(rendererOrigin);
  });

  it("allow the renderer origin to read responses", async () => {
    const response = await newApp().request("/desktop/status", {
      headers: { ...bearer(desktopToken), Origin: rendererOrigin },
    });
    expect(response.headers.get("Access-Control-Allow-Origin")).toBe(rendererOrigin);
  });

  it("give no CORS header to another origin", async () => {
    const response = await newApp().request("/desktop/status", {
      headers: { ...bearer(desktopToken), Origin: "http://localhost:3000" },
    });
    expect(response.headers.get("Access-Control-Allow-Origin")).toBeNull();
  });
});
