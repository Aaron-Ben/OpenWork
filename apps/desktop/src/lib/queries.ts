import {
  type AgentId,
  api,
  MESSAGE_PAGE_MAX,
  type MessageId,
  type RoomId,
  type RoomMessage,
  type TaskStatus,
  type TaskView,
} from "@crew/protocol";
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

/**
 * 在房间里发一条消息。带 `threadOf` 时发到那条消息的讨论串（第一次发时创建），返回的 `roomId` 是讨论串。
 */
export function useSendMessage(roomId: RoomId) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { body: string; threadOf?: MessageId }) =>
      server.call(api.desktop.sendMessage, { params: { roomId }, body: input }),
    onSuccess: async (posted, input) => {
      if (input.threadOf) void queryClient.invalidateQueries({ queryKey: queryKeys.threadList(roomId) });
      await fetchNewer(queryClient, posted.roomId);
    },
  });
}

/** 群聊里的讨论串。私聊没有讨论串，`enabled` 为 false 时不读取。 */
export function useThreads(roomId: RoomId, enabled = true) {
  return useQuery({
    queryKey: queryKeys.threadList(roomId),
    queryFn: () => server.call(api.desktop.listThreads, { params: { roomId } }),
    enabled,
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

/** 用户读到了房间（或讨论串）的第 `seq` 条。成功后刷新会话列表与讨论串摘要里的未读数。 */
export function useMarkRead(roomId: RoomId) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (seq: number) => server.call(api.desktop.markRead, { params: { roomId }, body: { seq } }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.conversations });
      void queryClient.invalidateQueries({ queryKey: queryKeys.threads });
    },
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

/** 运行记录列表。SSE 的 `run.activity` 让全部列表与详情一起失效。 */
export function useRuns(filter: { roomId?: RoomId; agentId?: AgentId }, enabled = true) {
  return useQuery({
    queryKey: queryKeys.runList(filter),
    queryFn: () => server.call(api.desktop.listRuns, { query: filter }),
    enabled,
  });
}

/** 一轮的全部内容。`runId` 为空时不读取。 */
export function useRun(runId: string | undefined) {
  return useQuery({
    queryKey: queryKeys.run(runId ?? ""),
    queryFn: () => server.call(api.desktop.getRun, { params: { runId: runId ?? "" } }),
    enabled: runId !== undefined,
  });
}

/** 房间里的任务，按编号排列。 */
export function useTasks(roomId: RoomId) {
  return useQuery({
    queryKey: queryKeys.tasks(roomId),
    queryFn: () => server.call(api.desktop.listTasks, { params: { roomId } }),
  });
}

/** 任务改动后：任务列表、讨论串摘要与房间的新消息（新建任务时发出的标题消息）。 */
function refreshAfterTask(queryClient: QueryClient, roomId: RoomId) {
  void queryClient.invalidateQueries({ queryKey: queryKeys.tasks(roomId) });
  void queryClient.invalidateQueries({ queryKey: queryKeys.threadList(roomId) });
  return fetchNewer(queryClient, roomId);
}

/** 任务操作：新建、把消息转成任务、改状态、换负责人。成功时返回改动后的任务。 */
export function useTaskActions(roomId: RoomId) {
  const queryClient = useQueryClient();
  const onSuccess = () => refreshAfterTask(queryClient, roomId);
  const create = useMutation({
    mutationFn: (input: { title: string; assigneeId?: AgentId }) =>
      server.call(api.desktop.createTask, { params: { roomId }, body: input }),
    onSuccess,
  });
  const convert = useMutation({
    mutationFn: (messageId: MessageId) =>
      server.call(api.desktop.convertToTask, { params: { roomId }, body: { messageId } }),
    onSuccess,
  });
  const setStatus = useMutation({
    mutationFn: ({ task, status }: { task: TaskView; status: TaskStatus }) =>
      server.call(api.desktop.setTaskStatus, { params: { roomId, number: task.number }, body: { status } }),
    onSuccess,
  });
  const assign = useMutation({
    mutationFn: ({ task, agentId }: { task: TaskView; agentId: AgentId | null }) =>
      server.call(api.desktop.assignTask, { params: { roomId, number: task.number }, body: { agentId } }),
    onSuccess,
  });
  return { create, convert, setStatus, assign };
}

/** Agent 的记忆文件。不存在时是 null。`enabled` 为 false（右栏没打开记忆）时不读取。 */
export function useMemory(agentId: string, enabled: boolean) {
  return useQuery({
    queryKey: queryKeys.memory(agentId),
    queryFn: () => window.crew.readMemory(agentId),
    enabled,
    staleTime: 0,
    refetchOnMount: "always",
  });
}
