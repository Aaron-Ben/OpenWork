import { type AgentId, api, MESSAGE_PAGE_MAX, type RoomId, type RoomMessage } from "@crew/protocol";
import { QueryClient, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { server } from "./api";
import { queryKeys } from "./keys";
import { MESSAGE_PAGE, mergeMessages, newestSeq } from "./messages";
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

export function useGroups() {
  return useQuery({
    queryKey: queryKeys.groups,
    queryFn: () => server.call(api.desktop.listGroups),
  });
}

/** 房间的消息。第一次取最新的一批；之后由 `fetchNewer` 与 `useLoadOlder` 往缓存里合并。 */
export function useMessages(roomId: RoomId) {
  return useQuery({
    queryKey: queryKeys.messages(roomId),
    queryFn: () => server.call(api.desktop.listMessages, { params: { roomId }, query: { limit: MESSAGE_PAGE } }),
  });
}

/**
 * 只取缓存里最新一条之后的消息并合并进去。一次取满上限时接着取，直到取完。
 * 缓存里还没有数据时，第一次读取可能正在进行、结果里没有这条新消息，所以让它重新开始；
 * 没有打开过这个房间时什么也不发生。
 */
export async function fetchNewer(queryClient: QueryClient, roomId: RoomId): Promise<void> {
  const key = queryKeys.messages(roomId);
  for (;;) {
    const cached = queryClient.getQueryData<RoomMessage[]>(key);
    if (!cached) {
      await queryClient.invalidateQueries({ queryKey: key });
      return;
    }
    const batch = await server.call(api.desktop.listMessages, {
      params: { roomId },
      query: { after: newestSeq(cached) },
    });
    queryClient.setQueryData<RoomMessage[]>(key, (current) => mergeMessages(current, batch));
    if (batch.length < MESSAGE_PAGE_MAX) return;
  }
}

/** 取缓存里最早一条之前的一批消息，合并到前面。 */
export function useLoadOlder(roomId: RoomId) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (before: number) =>
      server.call(api.desktop.listMessages, { params: { roomId }, query: { before, limit: MESSAGE_PAGE } }),
    onSuccess: (batch) =>
      queryClient.setQueryData<RoomMessage[]>(queryKeys.messages(roomId), (current) => mergeMessages(current, batch)),
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
    onSuccess: () => fetchNewer(queryClient, roomId),
  });
}

export function useCreateAgent() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: NewAgentInput) => server.call(api.desktop.createAgent, { body: input }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.agents });
      void queryClient.invalidateQueries({ queryKey: queryKeys.conversations });
    },
  });
}

export function useCreateGroup() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { name: string; agentIds: AgentId[] }) => server.call(api.desktop.createGroup, { body: input }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.groups });
      void queryClient.invalidateQueries({ queryKey: queryKeys.conversations });
    },
  });
}

export function useAddGroupMembers(roomId: RoomId) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (agentIds: AgentId[]) =>
      server.call(api.desktop.addGroupMembers, { params: { roomId }, body: { agentIds } }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.groups });
      void queryClient.invalidateQueries({ queryKey: queryKeys.conversations });
    },
  });
}

export function useConversations() {
  return useQuery({
    queryKey: queryKeys.conversations,
    queryFn: () => server.call(api.desktop.listConversations),
  });
}

/** 用户读到了房间的第 `seq` 条。成功后刷新会话列表里的未读数。 */
export function useMarkRead(roomId: RoomId) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (seq: number) => server.call(api.desktop.markRead, { params: { roomId }, body: { seq } }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: queryKeys.conversations }),
  });
}

/** 把一个 Agent 加进几个群聊：每个群聊调用一次加成员接口。 */
export function useJoinGroups(agentId: AgentId) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (roomIds: RoomId[]) => {
      for (const roomId of roomIds) {
        await server.call(api.desktop.addGroupMembers, { params: { roomId }, body: { agentIds: [agentId] } });
      }
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.groups });
      void queryClient.invalidateQueries({ queryKey: queryKeys.conversations });
    },
  });
}
