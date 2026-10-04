import { type BodyOf, type Endpoint, ErrorBody, endpointPath, type ParamsOf, type ResponseOf } from "./api";

// 按接口契约调用 Server 的客户端。界面与 Computer 共用：Node 与浏览器都有 fetch。

/** Server 拒绝了请求。`message` 是 Server 给出的原因，没有时用状态码说明。 */
export class ApiError extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

/** 从错误响应体 `{ error }` 里取出原因；格式不对时用状态码说明。 */
export function errorMessage(body: unknown, status: number): string {
  const parsed = ErrorBody.safeParse(body);
  return parsed.success ? parsed.data.error : `Server 返回 ${status}`;
}

export interface ApiClientOptions {
  baseUrl: string;
  token: string;
  fetch?: typeof fetch;
}

type CallInput<E extends Endpoint> = (ParamsOf<E> extends undefined ? unknown : { params: ParamsOf<E> }) &
  (BodyOf<E> extends undefined ? unknown : { body: BodyOf<E> });

/** 没有参数也没有请求体的接口不需要第二个参数。 */
export type CallArgs<E extends Endpoint> = [ParamsOf<E>, BodyOf<E>] extends [undefined, undefined]
  ? []
  : [input: CallInput<E>];

interface LooseInput {
  params?: Record<string, string>;
  body?: unknown;
}

export class ApiClient {
  private readonly fetchFn: typeof fetch;

  constructor(private readonly options: ApiClientOptions) {
    this.fetchFn = options.fetch ?? fetch;
  }

  get baseUrl(): string {
    return this.options.baseUrl;
  }

  /**
   * 调用一个接口，返回按响应 schema 校验后的值。Server 拒绝时抛出 `ApiError`；
   * 连不上 Server 时抛出 fetch 自己的错误。
   */
  call<E extends Endpoint>(endpoint: E, ...args: CallArgs<E>): Promise<ResponseOf<E>>;
  async call(endpoint: Endpoint, input?: LooseInput): Promise<unknown> {
    const url = new URL(endpointPath(endpoint.path, input?.params), this.options.baseUrl);
    // 浏览器的 fetch 只能以 window 为 this 调用：经 this.fetchFn(...) 调用会抛出 Illegal invocation，
    // 所以先取出来，当作普通函数调用。传入的 fetch 也可能是原生的，同样处理。
    const send = this.fetchFn;
    const response = await send(url, {
      method: endpoint.method,
      headers: {
        Authorization: `Bearer ${this.options.token}`,
        ...(input?.body === undefined ? {} : { "Content-Type": "application/json" }),
      },
      body: input?.body === undefined ? undefined : JSON.stringify(input.body),
    });
    if (!response.ok) {
      const body: unknown = await response.json().catch(() => undefined);
      throw new ApiError(response.status, errorMessage(body, response.status));
    }
    if (!endpoint.response) return undefined;
    return endpoint.response.parse(await response.json());
  }
}
