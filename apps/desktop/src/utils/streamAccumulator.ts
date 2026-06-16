import type { ChatItem } from '../type/chat'
import type { ChatStreamEventPayload } from '../type/providers'
import type { ContentBlock } from '../type/parts'

/// 纯函数:把一帧流式事件累积到 messages 里对应 `requestId` 的 assistant item。
/// 操作有序 `parts: ContentBlock[]`(而非旧的扁平字段),保留 text/thinking/tool 交错顺序。
/// 返回新数组(不可变);若无变化则原样返回。
export function applyEvent(
  messages: ChatItem[],
  payload: ChatStreamEventPayload,
  requestId: string,
): ChatItem[] {
  // 确保 requestId 对应的 assistant item 存在。
  const exists = messages.some((item) => item.id === requestId)
  const base: ChatItem[] = exists
    ? messages
    : [...messages, { id: requestId, role: 'assistant', parts: [], isStreaming: true }]

  switch (payload.event) {
    case 'text_delta': {
      const delta = payload.delta ?? ''
      if (!delta) return messages
      return mapAssistant(base, requestId, (item) => appendToPart(item, 'text', delta))
    }
    case 'reasoning_delta': {
      const delta = payload.delta ?? ''
      if (!delta) return messages
      return mapAssistant(base, requestId, (item) => appendToPart(item, 'thinking', delta))
    }
    case 'tool_call_start': {
      const id = payload.toolCallId
      if (!id) return messages
      return mapAssistant(base, requestId, (item) => {
        if (item.parts.some((part) => part.type === 'tool_call' && part.id === id)) return item
        const parts: ContentBlock[] = [
          ...item.parts,
          { type: 'tool_call', id, name: payload.toolName ?? '', input: '', state: 'pending' },
        ]
        return { ...item, parts }
      })
    }
    case 'tool_call_delta': {
      const id = payload.toolCallId
      if (!id) return messages
      return mapAssistant(base, requestId, (item) => ({
        ...item,
        parts: item.parts.map((part) =>
          part.type === 'tool_call' && part.id === id
            ? { ...part, input: part.input + (payload.partialInput ?? '') }
            : part,
        ),
      }))
    }
    case 'tool_call_end': {
      const id = payload.toolCallId
      if (!id) return messages
      return mapAssistant(base, requestId, (item) => ({
        ...item,
        parts: item.parts.map((part) =>
          part.type === 'tool_call' && part.id === id ? { ...part, state: 'finished' } : part,
        ),
      }))
    }
    case 'tool_result': {
      const id = payload.toolCallId
      if (!id) return messages
      const result: ContentBlock = {
        type: 'tool_result',
        id,
        name: payload.toolName ?? '',
        output: [{ type: 'text', text: payload.toolOutput ?? '' }],
        state: payload.isError ? 'error' : 'success',
      }
      return mapAssistant(base, requestId, (item) => ({ ...item, parts: [...item.parts, result] }))
    }
    case 'error': {
      return mapAssistant(base, requestId, (item) => ({
        ...item,
        isStreaming: false,
        parts: [
          ...item.parts,
          { type: 'text', text: `Request failed: ${payload.message ?? 'Unexpected error'}` },
        ],
      }))
    }
    default:
      return messages
  }
}

function mapAssistant(
  messages: ChatItem[],
  requestId: string,
  fn: (item: ChatItem) => ChatItem,
): ChatItem[] {
  return messages.map((item) => (item.id === requestId ? fn(item) : item))
}

/// 若最后一个指定类型的 part 存在则追加文本,否则新建一个 part。保证顺序。
function appendToPart(item: ChatItem, kind: 'text' | 'thinking', delta: string): ChatItem {
  const parts = [...item.parts]
  for (let i = parts.length - 1; i >= 0; i -= 1) {
    if (parts[i].type === kind) {
      const existing = parts[i] as { text?: string; thinking?: string }
      if (kind === 'text') {
        parts[i] = { type: 'text', text: (existing.text ?? '') + delta }
      } else {
        parts[i] = { type: 'thinking', thinking: (existing.thinking ?? '') + delta }
      }
      return { ...item, parts }
    }
  }
  parts.push(kind === 'text' ? { type: 'text', text: delta } : { type: 'thinking', thinking: delta })
  return { ...item, parts }
}
