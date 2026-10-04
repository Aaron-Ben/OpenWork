import { ApiClient } from "@crew/protocol";

/** 页面唯一的 Server 客户端。地址与凭证由 preload 经 `window.crew` 交来；接口与类型来自 protocol 的契约。 */
export const server = new ApiClient({ baseUrl: window.crew.serverUrl, token: window.crew.desktopToken });
