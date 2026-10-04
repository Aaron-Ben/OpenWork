import { api, type RoomId } from "@crew/protocol";
import { QueryClient, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { server } from "./api";
import { queryKeys } from "./keys";
import type { NewAgentInput } from "./new-agent";

/**
 * 数据只在两种时候重新获取：SSE 提示某部分变了，或 SSE 重连成功（见 events.ts）。
 * 所以缓存不按时间过期，切回窗口也不重新获取。
 */
export function createQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: { staleTime: Number.POSITIVE_INFINITY, refetchOnWindowFocus: false, retry: 1 },
    },
  });
}

export function useAgents() {
  return useQuery({
    queryKey: queryKeys.agents,
    queryFn: () => server.call(api.desktop.listAgents),
  });
}

export function useMessages(roomId: RoomId) {
  return useQuery({
    queryKey: queryKeys.messages(roomId),
    queryFn: () => server.call(api.desktop.listMessages, { params: { roomId } }),
  });
}

/**
 * 每次打开“新建 agent”时重新获取。Computer 启动后要几秒才上报模型，期间对话框可能已经打开，
 * 上报时的 SSE 提示 `models` 让它自动刷新。
 */
export function useModels(enabled: boolean) {
  return useQuery({
    queryKey: queryKeys.models,
    queryFn: () => server.call(api.desktop.listModels),
    enabled,
    staleTime: 0,
    refetchOnMount: "always",
  });
}

export function useSendMessage(roomId: RoomId) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: string) => server.call(api.desktop.sendMessage, { params: { roomId }, body: { body } }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: queryKeys.messages(roomId) }),
  });
}

export function useCreateAgent() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: NewAgentInput) => server.call(api.desktop.createAgent, { body: input }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: queryKeys.agents }),
  });
}
