import { create } from 'zustand'

/// 一个待审批的工具调用提示。`sessionId` 用于多会话隔离。
export interface ApprovalPrompt {
  id: string
  turnId: string
  sessionId: string
  toolRunId: string
  toolName: string
  input: unknown
  reason: string
}

interface ApprovalStoreState {
  pending: ApprovalPrompt[]
  push: (prompt: ApprovalPrompt) => void
  remove: (id: string) => void
  removeByTurn: (turnId: string) => void
  replaceSession: (sessionId: string, prompts: ApprovalPrompt[]) => void
}

/// 全局审批队列:agent loop 发出 `approval_request` 时 push(带 sessionId),
/// `ApprovalDialog` 只渲染当前活跃 session 的 pending,用户确认后 remove 并回传 `resolve_approval`。
export const useApprovalStore = create<ApprovalStoreState>((set) => ({
  pending: [],
  push: (prompt) =>
    set((state) =>
      state.pending.some((item) => item.id === prompt.id)
        ? state
        : { pending: [...state.pending, prompt] },
    ),
  remove: (id) =>
    set((state) => ({ pending: state.pending.filter((item) => item.id !== id) })),
  removeByTurn: (turnId) =>
    set((state) => ({ pending: state.pending.filter((item) => item.turnId !== turnId) })),
  replaceSession: (sessionId, prompts) =>
    set((state) => ({
      pending: [
        ...state.pending.filter((item) => item.sessionId !== sessionId),
        ...prompts.filter(
          (prompt, index, items) => items.findIndex((item) => item.id === prompt.id) === index,
        ),
      ],
    })),
}))
