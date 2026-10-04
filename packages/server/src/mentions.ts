import { HANDLE_MAX, mentionPattern, normalizeHandle } from "@crew/protocol";

// 从消息正文里找出 @ 到的 handle。纯函数：只看文本，是否是房间成员由调用方判断。

/** 代码块与行内代码：其中的 `@` 是代码的一部分，不是点名。 */
const CODE = /```[\s\S]*?(?:```|$)|~~~[\s\S]*?(?:~~~|$)|`[^`\n]*`/g;

/** 正文里 @ 到的 handle，小写、去重，按第一次出现的顺序。 */
export function mentionedHandles(body: string): string[] {
  const text = body.replace(CODE, " ");
  const handles = new Set<string>();
  for (const match of text.matchAll(mentionPattern())) {
    const handle = normalizeHandle(match[1] ?? "");
    if (handle.length > 0 && handle.length <= HANDLE_MAX) handles.add(handle);
  }
  return [...handles];
}
