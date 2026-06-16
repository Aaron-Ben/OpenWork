import { useEffect } from 'react'

import { providersApi } from '../api/providers'
import { useApprovalStore } from '../stores/approvalStore'
import { useSessionStore } from '../stores/sessionStore'

/// 全局单订阅 `chat-stream-event`(生命周期 = 挂载组件 = app)。
/// 按 `payload.sessionId` 分派:approval_request 入 approvalStore,done 触发 reload,
/// 其余累积到对应 session 的 messages。切 session 不重建监听,旧 session 流式不丢。
export function useChatStreamListener() {
  useEffect(() => {
    let disposed = false
    let unlisten: (() => void) | null = null

    void providersApi.listenToChatStream((payload) => {
      if (disposed) return
      const sessionId = payload.sessionId
      if (!sessionId) return

      if (payload.event === 'approval_request' && payload.approvalId) {
        useApprovalStore.getState().push({
          id: payload.approvalId,
          sessionId,
          toolName: payload.toolName ?? '',
          input: payload.input ?? null,
        })
        return
      }

      if (payload.event === 'done') {
        void useSessionStore.getState().reload(sessionId)
        return
      }

      useSessionStore.getState().applyStreamEvent(sessionId, payload)
    }).then((dispose) => {
      if (disposed) {
        dispose()
      } else {
        unlisten = dispose
      }
    })

    return () => {
      disposed = true
      unlisten?.()
    }
  }, [])
}
