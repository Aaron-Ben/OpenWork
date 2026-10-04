import { HANDLE_MAX } from "@crew/protocol";

// 从消息正文里找出 @ 到的 handle。纯函数：只看文本，是否是房间成员由调用方判断。

/** 代码块与行内代码：其中的 `@` 是代码的一部分，不是点名。 */
const CODE = /```[\s\S]*?(?:```|$)|~~~[\s\S]*?(?:~~~|$)|`[^`\n]*`/g;

/**
 * `@` 前面不能是字母、数字或这几个符号，否则是邮箱或路径的一部分，例如 `a@b.com`。
 * handle 的大小写不敏感：`@Alice` 也点名 `alice`。
 */
const MENTION = /(?<![A-Za-z0-9_.@/-])@([A-Za-z0-9][A-Za-z0-9-]*)/g;

/** 正文里 @ 到的 handle，小写、去重，按第一次出现的顺序。 */
export function mentionedHandles(body: string): string[] {
  const text = body.replace(CODE, " ");
  const handles = new Set<string>();
  for (const match of text.matchAll(MENTION)) {
    // 句末的 `-` 不属于 handle，例如 “@alice-请看”。
    const handle = (match[1] ?? "").toLowerCase().replace(/-+$/, "");
    if (handle.length > 0 && handle.length <= HANDLE_MAX) handles.add(handle);
  }
  return [...handles];
}
