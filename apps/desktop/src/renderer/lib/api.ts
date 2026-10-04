import { createServerClient } from "./server";

/** 页面唯一的 Server 客户端。地址与凭证由 preload 经 `window.crew` 交来。 */
export const api = createServerClient(window.crew.serverUrl, window.crew.desktopToken);
