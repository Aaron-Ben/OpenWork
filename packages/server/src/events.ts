import type { ComputerEvent, DesktopEvent } from "@crew/protocol";

type Listener<T> = (event: T) => void;

/** 一个事件通道。订阅者抛出的异常只记录，不影响其他订阅者，也不影响发布方。 */
export class Channel<T> {
  private readonly listeners = new Set<Listener<T>>();

  /** @returns 取消订阅的函数。 */
  subscribe(listener: Listener<T>): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  publish(event: T): void {
    for (const listener of this.listeners) {
      try {
        listener(event);
      } catch (error) {
        console.error("[server] 事件订阅者出错:", error);
      }
    }
  }
}

/**
 * Server 进程内的事件总线，把“数据变了”传给 SSE 连接。
 *
 * 只在单个 Server 进程内有效。Server 改为多实例部署时，换成 Redis pub/sub。
 */
export class EventHub {
  readonly desktop = new Channel<DesktopEvent>();
  readonly computer = new Channel<ComputerEvent>();
}
