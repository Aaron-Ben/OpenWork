import { useEffect, useState } from 'react'
import { ArrowLeft, LoaderCircle } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { headerInsetClass, SidebarReveal } from '@/app/SidebarReveal'
import { coreCommands } from '@/bridge/commands'
import type { RuntimeLoadedSession, RuntimeStoredMessage } from '@/bridge/compat'
import { resolveErrorMessage } from '@/lib/commandError'
import { agentInitial, formatElapsedClock, formatTokenCount } from './agentPresentation'
import type { AgentRailItem } from './agentRailModel'
import { AssistantMessage } from './components/AssistantMessage'
import { ToolActivityList } from './components/ToolActivityList'
import { UserMessage } from './components/UserMessage'

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
          <div className="flex items-start gap-3 border-b border-line pb-5">
            <span
              aria-hidden="true"
              className={`grid size-10 shrink-0 place-items-center rounded-full text-sm font-semibold ${
                item.orchestrator ? 'bg-clay text-paper' : 'bg-clay-soft text-clay'
              }`}
            >
              {agentInitial(item.role)}
            </span>
            <div className="min-w-0 flex-1">
              <h1 className="truncate font-sans text-lg font-semibold text-ink">{item.role}</h1>
              <p className="mt-0.5 truncate font-mono text-xs text-ink-faint">{item.task}</p>
            </div>
            <dl className="flex shrink-0 gap-6 text-right">
              <div>
                <dt className="text-[11px] text-ink-faint">{t('chat.agents.statusLabel')}</dt>
                <dd className="mt-0.5 text-sm text-ink-soft">
                  {t(`chat.subAgents.status.${item.status}`)}
                </dd>
              </div>
              <div>
                <dt className="text-[11px] text-ink-faint">{t('chat.agents.durationLabel')}</dt>
                <dd className="mt-0.5 font-mono text-sm tabular-nums text-ink-soft">
                  {`${formatElapsedClock(item.elapsedMs)} · ${formatTokenCount(item.tokens)}`}
                </dd>
              </div>
            </dl>
          </div>

          <div className="pt-6">
            {detail.state === 'loaded' ? (
              <ReadonlySubAgentTranscript messages={detail.session.messages} />
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
 * 不能复用 TranscriptMessage：它吃的是 ChatItem（字段 parts），而 loadSession 给的是
 * RuntimeStoredMessage（字段 content）；它还要求 Trace、撤销文件、重新应用这些交互回调，
 * 而这一页不该暴露任何交互。所以这里只按角色分派到三个展示组件。
 */
function ReadonlySubAgentTranscript({ messages }: { messages: RuntimeStoredMessage[] }) {
  const { t } = useTranslation()
  if (messages.length === 0) {
    return <p className="text-xs text-ink-faint">{t('chat.subAgents.emptyTranscript')}</p>
  }
  return (
    <div className="flex flex-col gap-4" data-readonly-sub-agent-transcript="true">
      {messages.map((message) => (
        <div key={message.id} className="min-w-0">
          {message.role === 'user' ? (
            <UserMessage parts={message.content} />
          ) : message.role === 'tool' ? (
            <ToolActivityList parts={message.content} />
          ) : (
            <AssistantMessage parts={message.content} />
          )}
        </div>
      ))}
    </div>
  )
}
