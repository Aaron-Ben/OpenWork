import type { ChatItem } from '../../type/chat'

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

  return merged
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
