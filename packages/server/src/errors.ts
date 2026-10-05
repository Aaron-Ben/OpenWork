import type { MuteRefusal, ReminderRefusal, TaskRefusal } from "@crew/protocol";

/** 业务层拒绝一个请求。路由把它转成 `{ error }` 与对应的状态码；任务、提醒与静音被拒绝时另带 `refusal`。 */
export class RequestError extends Error {
  constructor(
    readonly status: 400 | 403 | 404 | 409,
    message: string,
    readonly refusal?: TaskRefusal | ReminderRefusal | MuteRefusal,
  ) {
    super(message);
  }
}
