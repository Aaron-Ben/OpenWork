/** 判断输入框里的一次按键是否应该发送消息所需的字段，与 React 的 KeyboardEvent 兼容。 */
export interface ComposerKey {
  key: string;
  shiftKey: boolean;
  /** 输入法正在组字（例如拼音选词）时为 true。 */
  isComposing: boolean;
  /** 部分输入法在组字时只给出 keyCode 229。 */
  keyCode: number;
}

/** Enter 发送，Shift+Enter 换行；输入法组字时按下的 Enter 用来选词，不发送。 */
export function shouldSend(event: ComposerKey): boolean {
  return event.key === "Enter" && !event.shiftKey && !event.isComposing && event.keyCode !== 229;
}

/** 正文去掉首尾空白后为空时不能发送，与 Server 的校验一致。 */
export function canSend(draft: string): boolean {
  return draft.trim().length > 0;
}
