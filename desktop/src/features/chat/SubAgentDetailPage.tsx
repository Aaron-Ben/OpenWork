import { useEffect, useMemo, useState } from 'react'
import { ArrowLeft, LoaderCircle } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { headerInsetClass, SidebarReveal } from '@/app/SidebarReveal'
import { coreCommands } from '@/bridge/commands'
import type { RuntimeLoadedSession, RuntimeStoredMessage } from '@/bridge/compat'
import { resolveErrorMessage } from '@/lib/commandError'
import type { AgentRailItem } from './agentRailModel'
import { AssistantMessage } from './components/AssistantMessage'
import { ToolActivityList } from './components/ToolActivityList'
import { UserMessage } from './components/UserMessage'
import { buildReadonlyTranscript } from './transcript'

export type SubAgentDetailState =
  | { state: 'loading' }
  | { state: 'loaded'; session: RuntimeLoadedSession }
  | { state: 'error'; message: string }

export function loadSubAgentTranscript(childSessionId: string): Promise<RuntimeLoadedSession> {
  return coreCommands.loadSession(childSessionId)
}

interface SubAgentDetailPageProps {
  item: AgentRailItem
  sessionTitle: string
  /** 这一页会整个占掉中栏，所以顶栏的红绿灯让位与展开入口都得自己带上。 */
  sidebarExpanded: boolean
  onToggleSidebar: () => void
  onBack: () => void
}

/**
 * 子智能体详情：只读。
 * 这里不提供"重跑"或"直接下发"——越过主控给子 Agent 发指令会让主控的任务图与实际执行脱节。
 */
export function SubAgentDetailPage({
  item,
  sessionTitle,
  sidebarExpanded,
  onToggleSidebar,
  onBack,
}: SubAgentDetailPageProps) {
  const { t } = useTranslation()
  const detail = useSubAgentDetail(item.sessionId)

  return (
    <div className="flex h-full min-h-0 flex-col bg-paper">
      <header
        data-sub-agent-header="true"
        data-tauri-drag-region="deep"
        className={`flex h-14 shrink-0 items-center gap-3 border-b border-line ${headerInsetClass(sidebarExpanded)}`}
      >
        <SidebarReveal sidebarExpanded={sidebarExpanded} onToggleSidebar={onToggleSidebar} />
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="shrink-0 gap-1.5 text-ink-soft"
          onClick={onBack}
        >
          <ArrowLeft size={15} />
          {t('chat.agents.backToMain')}
        </Button>
        <p className="min-w-0 truncate text-xs text-ink-faint">
          {`${sessionTitle} / ${t('chat.agents.subAgent')}`}
        </p>
      </header>

      <div className="min-h-0 flex-1 overflow-auto">
        <div className="mx-auto w-full max-w-4xl px-6 py-6 pb-10 max-[560px]:px-4">
          <div>
            {detail.state === 'loaded' ? (
              <ReadonlySubAgentTranscript
                messages={detail.session.messages}
                workspaceRoot={detail.session.session.workingDirectory}
              />
            ) : detail.state === 'error' ? (
              <p className="text-xs text-status-danger-ink" role="alert">
                {t('chat.subAgents.transcriptFailed', { reason: detail.message })}
              </p>
            ) : (
              <p className="inline-flex items-center gap-2 text-xs text-ink-faint" role="status">
                <LoaderCircle size={13} className="animate-spin" />
                {t('chat.subAgents.loadingTranscript')}
              </p>
            )}
          </div>
        </div>
      </div>
    </div>
  )
}

function useSubAgentDetail(childSessionId: string): SubAgentDetailState {
  const [detail, setDetail] = useState<SubAgentDetailState>({ state: 'loading' })

  useEffect(() => {
    let active = true
    setDetail({ state: 'loading' })
    void loadSubAgentTranscript(childSessionId)
      .then((session) => {
        if (active) setDetail({ state: 'loaded', session })
      })
      .catch((error) => {
        if (active) setDetail({ state: 'error', message: resolveErrorMessage(error) })
      })
    return () => {
      active = false
    }
  }, [childSessionId])

  return detail
}

/**
 * 落库消息的只读渲染。
 *
 * 投影必须走 buildReadonlyTranscript：Core 把一次工具调用拆成 assistant 的 tool_call 与
 * Role::Tool 的 tool_result 两条消息落库，而配对只在单个 parts 数组内进行。直接按角色
 * 逐条分派会把两半丢进不同的 ToolActivityList，调用那一行就永远停在 submitted 转圈。
 *
 * 不能复用 TranscriptMessage：它要求 Trace、撤销文件、重新应用这些交互回调，
 * 而这一页不该暴露任何交互。所以这里只按角色分派到三个展示组件。
 */
export function ReadonlySubAgentTranscript({
  messages,
  workspaceRoot,
}: {
  messages: RuntimeStoredMessage[]
  workspaceRoot?: string
}) {
  const { t } = useTranslation()
  const items = useMemo(() => buildReadonlyTranscript(messages), [messages])
  if (items.length === 0) {
    return <p className="text-xs text-ink-faint">{t('chat.subAgents.emptyTranscript')}</p>
  }
  return (
    <div className="flex flex-col gap-4" data-readonly-sub-agent-transcript="true">
      {items.map((item) => (
        <div key={item.id} className="min-w-0">
          {item.role === 'user' ? (
            <UserMessage parts={item.parts} />
          ) : item.role === 'tool' ? (
            // 折叠后仍留在这里的，是找不到对应 tool_call 的孤儿结果。
            <ToolActivityList parts={item.parts} workspaceRoot={workspaceRoot} />
          ) : (
            <AssistantMessage parts={item.parts} workspaceRoot={workspaceRoot} />
          )}
        </div>
      ))}
    </div>
  )
}
