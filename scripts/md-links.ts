import { existsSync } from "node:fs";
import { dirname, resolve } from "node:path";

export interface BrokenLink {
  line: number;
  target: string;
}

/** 匹配行内链接 `[文字](目标)`，目标后可以带一个用引号包住的标题。 */
const LINK = /\[[^\]]*\]\(<?([^)\s>]+)>?(?:\s+"[^"]*")?\)/g;
/** 不指向本仓库文件的链接。 */
const EXTERNAL = /^(?:[a-z][a-z0-9+.-]*:|#)/i;

/**
 * 找出 Markdown 中指向不存在文件的相对链接。
 *
 * 忽略外部链接、纯锚点、围栏代码块与行内代码。目标末尾的 `#锚点` 与 `:行号` 不参与检查。
 *
 * @param file Markdown 文件的绝对路径，相对链接以它所在的目录为基准。
 * @returns 每个失效链接的行号（从 1 开始）与原始目标。
 */
export function findBrokenLinks(
  file: string,
  content: string,
  exists: (path: string) => boolean = existsSync,
): BrokenLink[] {
  const broken: BrokenLink[] = [];
  let fence: string | undefined;
  content.split("\n").forEach((line, index) => {
    const marker = line.match(/^\s*(```|~~~)/)?.[1];
    if (marker) {
      fence = fence === marker ? undefined : (fence ?? marker);
      return;
    }
    if (fence) return;
    const text = line.replace(/`[^`]*`/g, "");
    for (const match of text.matchAll(LINK)) {
      const target = match[1];
      if (!target || EXTERNAL.test(target)) continue;
      const path = decodeURI(target.replace(/#.*$/, "").replace(/:\d+(?:-\d+)?$/, ""));
      if (path && !exists(resolve(dirname(file), path))) {
        broken.push({ line: index + 1, target });
      }
    }
  });
  return broken;
}
