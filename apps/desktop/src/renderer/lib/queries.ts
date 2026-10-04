import type { RoomId } from "@crew/protocol";
import { QueryClient, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "./api";
import { queryKeys } from "./keys";
import type { NewAgentInput } from "./new-agent";
import { request } from "./server";

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
    queryFn: async () => request(api.desktop.agents.$get()),
  });
}

export function useMessages(roomId: RoomId) {
  return useQuery({
    queryKey: queryKeys.messages(roomId),
    queryFn: async () => request(api.desktop.rooms[":roomId"].messages.$get({ param: { roomId } })),
  });
}

/** 模型列表没有对应的 SSE 提示，每次打开“新建 agent”时重新获取。 */
export function useModels(enabled: boolean) {
  return useQuery({
    queryKey: queryKeys.models,
    queryFn: async () => request(api.desktop.models.$get()),
    enabled,
    staleTime: 0,
    refetchOnMount: "always",
  });
}

export function useSendMessage(roomId: RoomId) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (body: string) =>
      request(api.desktop.rooms[":roomId"].messages.$post({ param: { roomId }, json: { body } })),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: queryKeys.messages(roomId) }),
  });
}

export function useCreateAgent() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (input: NewAgentInput) => request(api.desktop.agents.$post({ json: input })),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: queryKeys.agents }),
  });
}
