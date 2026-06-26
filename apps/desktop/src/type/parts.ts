// 镜像 openwork-protocol 的 ContentBlock serde(tag="type", rename_all="snake_case")。
// 字段保持 snake_case 以与后端 JSON 一致,避免来回转换。

export type ToolCallState = 'pending' | 'submitted' | 'finished'

export type ToolResultState = 'success' | 'error' | 'interrupted' | 'denied' | 'running'

export interface TextBlock {
  type: 'text'
  text: string
}

export interface ThinkingBlock {
  type: 'thinking'
  thinking: string
}

export type DataSource =
  | { source_type: 'url'; url: string; media_type: string }
  | { source_type: 'base64'; data: string; media_type: string }
  | { source_type: 'file_id'; id: string }

export interface DataBlock {
  type: 'data'
  source: DataSource
  name?: string | null
}

export interface ToolCallBlock {
  type: 'tool_call'
  id: string
  name: string
  input: string
  state: ToolCallState
}

export interface ToolResultBlock {
  type: 'tool_result'
  id: string
  name: string
  output: ContentBlock[]
  state: ToolResultState
}

export type ContentBlock = TextBlock | ThinkingBlock | DataBlock | ToolCallBlock | ToolResultBlock

/// 从 ContentBlock 序列里抽出所有文本(拼接),用于把 tool_result.output 转成可显示字符串。
export function extractText(blocks: ContentBlock[]): string {
  return blocks
    .filter((block): block is TextBlock => block.type === 'text')
    .map((block) => block.text)
    .join('\n')
}
