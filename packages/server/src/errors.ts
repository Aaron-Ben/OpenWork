/** 业务层拒绝一个请求。路由把它转成 `{ error }` 与对应的状态码。 */
export class RequestError extends Error {
  constructor(
    readonly status: 400 | 403 | 404 | 409,
    message: string,
  ) {
    super(message);
  }
}
