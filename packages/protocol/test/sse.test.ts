import { describe, expect, it } from "vitest";
import { z } from "zod";
import { runEventStream } from "../src";

// 用可控的假 fetch 测试 SSE 读取与重连：每次调用返回一个由测试写入、关闭的响应流。

const Event = z.object({ type: z.literal("ping"), n: z.number() });
type Event = z.infer<typeof Event>;

const fastBackoff = { initialMs: 5, maxMs: 40, resetAfterMs: 10_000 };

/** 一个连接：测试向它写入 SSE 文本，或关闭它模拟断线。 */
class FakeConnection {
  private controller!: ReadableStreamDefaultController<Uint8Array>;
  readonly body = new ReadableStream<Uint8Array>({
    start: (controller) => {
      this.controller = controller;
    },
  });
  send(text: string) {
    this.controller.enqueue(new TextEncoder().encode(text));
  }
  close() {
    this.controller.close();
  }
}

/** 按顺序返回预设的响应；记录每次调用时间与请求头。 */
function fakeFetch(responses: Array<() => Response>) {
  const calls: Array<{ at: number; headers: Headers }> = [];
  const fetchFn: typeof fetch = async (_input, init) => {
    calls.push({ at: Date.now(), headers: new Headers(init?.headers) });
    const next = responses.shift();
    if (!next) return new Promise<Response>(() => {});
    return next();
  };
  return { fetchFn, calls };
}

function sse(connection: FakeConnection) {
  return () => new Response(connection.body, { headers: { "Content-Type": "text/event-stream" } });
}

const until = async (check: () => boolean) => {
  for (let i = 0; i < 200 && !check(); i++) await new Promise((r) => setTimeout(r, 5));
  if (!check()) throw new Error("等待超时");
};

describe("runEventStream", () => {
  it("delivers parsed events and sends the authorization header", async () => {
    const connection = new FakeConnection();
    const { fetchFn, calls } = fakeFetch([sse(connection)]);
    const events: Event[] = [];
    const controller = new AbortController();
    const done = runEventStream({
      url: "http://server/events",
      headers: { Authorization: "Bearer token" },
      schema: Event,
      onEvent: (e) => events.push(e),
      signal: controller.signal,
      fetch: fetchFn,
      backoff: fastBackoff,
    });

    connection.send('data: {"type":"ping","n":1}\n\n: keepalive\n\ndata: {"type":"ping","n":2}\n\n');
    await until(() => events.length === 2);
    controller.abort();
    await done;

    expect(events).toEqual([
      { type: "ping", n: 1 },
      { type: "ping", n: 2 },
    ]);
    expect(calls[0]?.headers.get("Authorization")).toBe("Bearer token");
    expect(calls[0]?.headers.get("Accept")).toBe("text/event-stream");
  });

  it("reconnects after the stream ends and calls onOpen for every connection", async () => {
    const first = new FakeConnection();
    const second = new FakeConnection();
    const { fetchFn, calls } = fakeFetch([sse(first), sse(second)]);
    let opens = 0;
    const controller = new AbortController();
    const done = runEventStream({
      url: "http://server/events",
      headers: {},
      schema: Event,
      onEvent: () => {},
      onOpen: () => opens++,
      signal: controller.signal,
      fetch: fetchFn,
      backoff: fastBackoff,
    });

    await until(() => opens === 1);
    first.close();
    await until(() => opens === 2);
    controller.abort();
    await done;

    expect(calls).toHaveLength(2);
  });

  it("keeps the stream open when an event does not match the schema", async () => {
    const connection = new FakeConnection();
    const { fetchFn } = fakeFetch([sse(connection)]);
    const events: Event[] = [];
    const errors: unknown[] = [];
    const controller = new AbortController();
    const done = runEventStream({
      url: "http://server/events",
      headers: {},
      schema: Event,
      onEvent: (e) => events.push(e),
      onError: (e) => errors.push(e),
      signal: controller.signal,
      fetch: fetchFn,
      backoff: fastBackoff,
    });

    connection.send('data: {"type":"other"}\n\ndata: not json\n\ndata: {"type":"ping","n":3}\n\n');
    await until(() => events.length === 1);
    controller.abort();
    await done;

    expect(events).toEqual([{ type: "ping", n: 3 }]);
    expect(errors).toHaveLength(2);
  });

  it("doubles the wait between failed attempts up to the maximum", async () => {
    const failing = () => new Response("no", { status: 503 });
    const { fetchFn, calls } = fakeFetch([failing, failing, failing, failing, failing]);
    const controller = new AbortController();
    const done = runEventStream({
      url: "http://server/events",
      headers: {},
      schema: Event,
      onEvent: () => {},
      signal: controller.signal,
      fetch: fetchFn,
      backoff: { initialMs: 20, maxMs: 60, resetAfterMs: 10_000 },
    });

    await until(() => calls.length === 5);
    controller.abort();
    await done;

    const gaps = calls.slice(1).map((call, i) => call.at - (calls[i]?.at ?? 0));
    // 等待时间依次是 20、40、60（封顶）、60 毫秒。
    expect(gaps[0]).toBeGreaterThanOrEqual(18);
    expect(gaps[1]).toBeGreaterThanOrEqual(38);
    expect(gaps[2]).toBeGreaterThanOrEqual(58);
    expect(gaps[3]).toBeLessThan(120);
  });

  it("returns promptly when aborted while waiting to reconnect", async () => {
    const { fetchFn } = fakeFetch([() => new Response("no", { status: 503 })]);
    const controller = new AbortController();
    const done = runEventStream({
      url: "http://server/events",
      headers: {},
      schema: Event,
      onEvent: () => {},
      signal: controller.signal,
      fetch: fetchFn,
      backoff: { initialMs: 60_000, maxMs: 60_000, resetAfterMs: 60_000 },
    });

    await new Promise((r) => setTimeout(r, 20));
    const before = Date.now();
    controller.abort();
    await done;
    expect(Date.now() - before).toBeLessThan(100);
  });
});

describe("runEventStream when aborted early", () => {
  it("returns when aborted right after the connection opens", async () => {
    const connection = new FakeConnection();
    const controller = new AbortController();
    const fetchFn: typeof fetch = async () => {
      // 在返回响应之前中止：读取开始时信号已经是中止状态。
      controller.abort();
      return new Response(connection.body);
    };
    const before = Date.now();
    await runEventStream({
      url: "http://server/events",
      headers: {},
      schema: Event,
      onEvent: () => {},
      signal: controller.signal,
      fetch: fetchFn,
    });
    expect(Date.now() - before).toBeLessThan(500);
  });
});
