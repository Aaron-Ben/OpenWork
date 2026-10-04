import { createParser } from "eventsource-parser";
import type { Parser } from "./stdio";

// SSE 读取与重连。Computer（Node）与界面（浏览器）共用这一份实现：只用两边都有的 fetch 与流。
// 用 fetch 而不是 EventSource，是因为 EventSource 不能设置 Authorization 请求头。

export interface Backoff {
  /** 第一次重连前的等待时间。 */
  initialMs: number;
  /** 等待时间的上限。每次失败后翻倍，直到这个值。 */
  maxMs: number;
  /** 一次连接持续这么久后断开，下次重连的等待时间回到 `initialMs`。 */
  resetAfterMs: number;
}

export const DEFAULT_BACKOFF: Backoff = { initialMs: 1_000, maxMs: 30_000, resetAfterMs: 60_000 };

/** 单个事件允许缓冲的最大字符数。超过时断开连接并重连。 */
const MAX_EVENT_CHARS = 1024 * 1024;

export interface EventStreamOptions<T> {
  url: string;
  headers: Record<string, string>;
  /** 每个事件的 `data` 是一段 JSON，用它校验。 */
  schema: Parser<T>;
  onEvent(event: T): void;
  /**
   * 每次连接成功后调用，包括重连。断线期间的事件不会补发，调用方在这里重新读取完整状态。
   */
  onOpen?(): void;
  /** 连接失败、断开或收到无法解析的事件时调用。循环不会因此停止。 */
  onError?(error: unknown): void;
  /** 中止后循环结束，`runEventStream` 返回。 */
  signal: AbortSignal;
  fetch?: typeof fetch;
  backoff?: Backoff;
}

/**
 * 连接一个 SSE 接口并持续读取，断开后按指数退避重连，直到 `signal` 中止。
 *
 * 事件只是失效提示：丢失一个事件的代价是晚一点刷新，所以重连不补发，
 * 而是在 `onOpen` 里由调用方重新读取。
 */
export async function runEventStream<T>(options: EventStreamOptions<T>): Promise<void> {
  const backoff = options.backoff ?? DEFAULT_BACKOFF;
  const fetchFn = options.fetch ?? fetch;
  let failures = 0;

  while (!options.signal.aborted) {
    const startedAt = Date.now();
    try {
      await readOnce(fetchFn, options);
      if (options.signal.aborted) return;
      options.onError?.(new Error("SSE 连接已断开"));
    } catch (error) {
      if (options.signal.aborted) return;
      options.onError?.(error);
    }
    if (Date.now() - startedAt >= backoff.resetAfterMs) failures = 0;
    const delay = Math.min(backoff.initialMs * 2 ** failures, backoff.maxMs);
    failures += 1;
    await sleep(delay, options.signal);
  }
}

/** 建立一次连接并读到流结束。连接成功后调用 `onOpen`。 */
async function readOnce<T>(fetchFn: typeof fetch, options: EventStreamOptions<T>): Promise<void> {
  const response = await fetchFn(options.url, {
    headers: { ...options.headers, Accept: "text/event-stream" },
    signal: options.signal,
  });
  if (!response.ok || !response.body) {
    throw new Error(`SSE 连接失败：${response.status}`);
  }
  options.onOpen?.();

  let overflow: Error | undefined;
  const parser = createParser({
    maxBufferSize: MAX_EVENT_CHARS,
    onEvent: (message) => {
      try {
        options.onEvent(options.schema.parse(JSON.parse(message.data)));
      } catch (error) {
        options.onError?.(error);
      }
    },
    onError: (error) => {
      if (error.type === "max-buffer-size-exceeded") overflow = error;
      options.onError?.(error);
    },
  });

  const reader = response.body.pipeThrough(new TextDecoderStream()).getReader();
  // 不依赖 fetch 实现在中止时关闭响应流：中止时主动取消读取，`read()` 随即返回。
  const cancel = () => {
    void reader.cancel();
  };
  options.signal.addEventListener("abort", cancel, { once: true });
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) return;
      parser.feed(value);
      if (overflow) throw overflow;
    }
  } finally {
    options.signal.removeEventListener("abort", cancel);
    reader.releaseLock();
  }
}

/** 等待 `ms` 毫秒；`signal` 中止时立即返回。 */
function sleep(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    const timer = setTimeout(done, ms);
    signal.addEventListener("abort", done, { once: true });
    function done() {
      clearTimeout(timer);
      signal.removeEventListener("abort", done);
      resolve();
    }
  });
}
