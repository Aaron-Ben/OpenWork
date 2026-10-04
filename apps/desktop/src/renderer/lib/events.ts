import { DesktopEvent, runEventStream } from "@crew/protocol";
import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { keysForEvent } from "./keys";

export type Connection = "connecting" | "connected" | "reconnecting";

/**
 * 订阅 Server 的 SSE 失效提示，让对应的缓存重新获取，并返回连接状态。
 * 断线期间的提示不会补发，所以每次连接成功都让全部缓存失效。
 */
export function useServerEvents(): Connection {
  const queryClient = useQueryClient();
  const [connection, setConnection] = useState<Connection>("connecting");

  useEffect(() => {
    const controller = new AbortController();
    void runEventStream({
      url: new URL("/desktop/events", window.crew.serverUrl).toString(),
      headers: { Authorization: `Bearer ${window.crew.desktopToken}` },
      schema: DesktopEvent,
      signal: controller.signal,
      onOpen: () => {
        setConnection("connected");
        void queryClient.invalidateQueries();
      },
      onDisconnect: () => setConnection("reconnecting"),
      onEvent: (event) => {
        void queryClient.invalidateQueries({ queryKey: keysForEvent(event) });
      },
      onError: (error) => console.warn("[crew] SSE:", error),
    });
    return () => controller.abort();
  }, [queryClient]);

  return connection;
}
