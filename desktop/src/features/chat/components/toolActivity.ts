import type { ChatItem } from '@/types/chat'

/// 持久化后 Tool Message 与触发它的 Assistant Message 分开保存。
/// UI 将可关联的结果折叠回对应 Assistant，保证流式态和重载态都只显示一行 ToolRun。
export function mergeToolMessages(messages: ChatItem[]): ChatItem[] {
  const merged: ChatItem[] = []

  for (const message of messages) {
    if (message.role !== 'tool') {
      merged.push(message)
      continue
    }

    const remaining: ChatItem['parts'] = []
    for (const part of message.parts) {
      if (part.type !== 'tool_result') {
        remaining.push(part)
        continue
      }

      const assistantIndex = findMatchingAssistant(merged, part.id)
      if (assistantIndex < 0) {
        remaining.push(part)
        continue
      }

      const assistant = merged[assistantIndex]
      const alreadyMerged = assistant.parts.some(
        (candidate) => candidate.type === 'tool_result' && candidate.id === part.id,
      )
      if (!alreadyMerged) {
        merged[assistantIndex] = { ...assistant, parts: [...assistant.parts, part] }
      }
    }

    if (remaining.length > 0) {
      merged.push({ ...message, parts: remaining })
    }
  }

  const coalesced: ChatItem[] = []
  for (const message of merged) {
    const previous = coalesced[coalesced.length - 1]
    if (
      previous
      && previous.turnId === message.turnId
      && isDisplayActivityMessage(previous)
      && isDisplayActivityMessage(message)
    ) {
      coalesced[coalesced.length - 1] = {
        ...previous,
        parts: [...previous.parts, ...message.parts],
        sourceMessageIds: [
          ...(previous.sourceMessageIds ?? [previous.id]),
          ...(message.sourceMessageIds ?? [message.id]),
        ],
      }
    } else {
      coalesced.push(message)
    }
  }

  return coalesced
}

/// 文件工具始终保留在执行时间线上；Turn 完成后，再在最后一条文本回答后
/// 追加一条只负责汇总展示的派生消息。派生消息不写回数据库。
export function appendCompletedFileChangeSummaries(
  messages: ChatItem[],
  activeTurnId: string | null,
): ChatItem[] {
  const arranged = messages.map((message) => ({
    ...message,
    parts: [...message.parts],
    // Core 会拒绝活跃 Turn 上的 undo；显式投影 Turn 状态，避免把必失败的动作呈现为可用。
    turnActive: message.turnId != null && message.turnId === activeTurnId,
    fileChangePresentation: message.fileChangePresentation ?? ('activity' as const),
  }))
  const summariesAfter = new Map<number, ChatItem>()
  const turnIds = new Set(
    arranged.flatMap((message) => message.turnId ? [message.turnId] : []),
  )

  for (const turnId of turnIds) {
    if (turnId === activeTurnId) continue
    const turnIndexes = arranged.flatMap((message, index) =>
      message.turnId === turnId ? [index] : [],
    )
    let answerIndex: number | null = null
    for (let offset = turnIndexes.length - 1; offset >= 0; offset -= 1) {
      const index = turnIndexes[offset]
      if (
        arranged[index].role === 'assistant' && arranged[index].parts.some(
          (part) => part.type === 'text' && part.text.trim().length > 0,
        )
      ) {
        answerIndex = index
        break
      }
    }
    if (answerIndex == null) continue

    const fileChangeCallIds = new Set<string>()
    for (const index of turnIndexes) {
      for (const part of arranged[index].parts) {
        if (
          part.type === 'tool_result' &&
          part.artifacts?.some((artifact) => artifact.kind === 'file_change')
        ) {
          fileChangeCallIds.add(part.id)
        }
      }
    }
    if (fileChangeCallIds.size === 0) continue

    const summaryParts: ChatItem['parts'] = []
    for (const index of turnIndexes) {
      for (const part of arranged[index].parts) {
        if (
          (part.type === 'tool_call' || part.type === 'tool_result') &&
          fileChangeCallIds.has(part.id)
        ) {
          summaryParts.push(part)
        }
      }
    }
    if (summaryParts.length === 0) continue
    summariesAfter.set(answerIndex, {
      id: `${arranged[answerIndex].id}-file-change-summary`,
      turnId,
      role: 'assistant',
      parts: summaryParts,
      fileChangePresentation: 'summary',
    })
  }

  return arranged.flatMap((message, index) => {
    const summary = summariesAfter.get(index)
    return summary ? [message, summary] : [message]
  })
}

function findMatchingAssistant(messages: ChatItem[], toolCallId: string): number {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = messages[index]
    if (message.role === 'user') return -1
    if (
      message.role === 'assistant' &&
      message.parts.some((part) => part.type === 'tool_call' && part.id === toolCallId)
    ) {
      return index
    }
  }
  return -1
}

function isDisplayActivityMessage(message: ChatItem): boolean {
  if (message.role !== 'assistant' || message.parts.length === 0) return false
  return message.parts.every((part) =>
    (part.type === 'tool_call' || part.type === 'tool_result')
    && (
      part.name === 'read'
      || part.name === 'list'
      || part.name === 'glob'
      || part.name === 'grep'
      || part.name === 'edit'
      || part.name === 'write'
      || part.name === 'bash'
      || part.name === 'spawn_agent'
      || part.name === 'wait_agent'
    )
  )
}
