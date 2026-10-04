import type { WebContents } from "electron";

/**
 * 窗口里的一次导航应交给系统浏览器打开的地址；不该打开时返回 undefined。
 * 只放行 http 与 https，其余协议（file:、javascript: 等）一律丢弃。界面自己的页面不算外部链接。
 */
export function externalUrl(url: string, rendererOrigin: string): string | undefined {
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    // 无法解析的地址不打开，也不需要报告：它只可能来自消息里写坏的链接。
    return undefined;
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return undefined;
  if (parsed.origin === rendererOrigin) return undefined;
  return parsed.toString();
}

/**
 * 窗口只显示界面自己的页面：点消息里的链接时不让窗口跳走，而是交给系统浏览器。
 * 新窗口请求（`target="_blank"`）与页面内跳转都经过这里。
 */
export function confineNavigation(
  contents: WebContents,
  rendererOrigin: string,
  openExternal: (url: string) => Promise<void>,
): void {
  const open = (url: string) => {
    const target = externalUrl(url, rendererOrigin);
    if (target) void openExternal(target);
  };
  contents.setWindowOpenHandler(({ url }) => {
    open(url);
    return { action: "deny" };
  });
  contents.on("will-navigate", (event, url) => {
    if (new URL(url).origin === rendererOrigin) return;
    event.preventDefault();
    open(url);
  });
}
