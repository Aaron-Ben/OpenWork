import { mentionPattern, normalizeHandle } from "@crew/protocol";

// 消息正文里 @handle 的高亮。规则与 Server 记录点名相同（protocol 的 mentionPattern），
// 只高亮确实存在的 handle，代码与链接里的不处理。

export type Segment = { text: string } | { mention: string; text: string };

/** 把一段文字切成普通文字与 @handle。不在 `handles` 里的 @ 保持原样。 */
export function splitMentions(text: string, handles: ReadonlySet<string>): Segment[] {
  const segments: Segment[] = [];
  let last = 0;
  for (const match of text.matchAll(mentionPattern())) {
    const raw = match[1] ?? "";
    const handle = normalizeHandle(raw);
    if (!handles.has(handle)) continue;
    const start = match.index ?? 0;
    // 句末的 `-` 不属于 handle，留在普通文字里。
    const end = start + 1 + handle.length;
    if (start > last) segments.push({ text: text.slice(last, start) });
    segments.push({ mention: handle, text: text.slice(start, end) });
    last = end;
  }
  if (last < text.length) segments.push({ text: text.slice(last) });
  return segments;
}

/** rehype 语法树里用到的部分。只读写这几个字段，不依赖 hast 的类型包。 */
interface TreeNode {
  type: string;
  tagName?: string;
  value?: string;
  properties?: Record<string, unknown>;
  children?: TreeNode[];
}

const SKIPPED = new Set(["code", "pre", "a"]);

/** rehype 插件：把正文里的 @handle 包进 `<span class="mention">`。 */
export function rehypeMentions(options: { handles: ReadonlySet<string> }) {
  const visit = (node: TreeNode) => {
    if (!node.children) return;
    node.children = node.children.flatMap((child): TreeNode[] => {
      if (child.type === "element" && child.tagName && SKIPPED.has(child.tagName)) return [child];
      if (child.type !== "text") {
        visit(child);
        return [child];
      }
      return splitMentions(child.value ?? "", options.handles).map((segment) =>
        "mention" in segment
          ? {
              type: "element",
              tagName: "span",
              properties: { className: ["mention"] },
              children: [{ type: "text", value: segment.text }],
            }
          : { type: "text", value: segment.text },
      );
    });
  };
  return (tree: TreeNode) => visit(tree);
}
