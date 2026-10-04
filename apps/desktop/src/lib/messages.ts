import type { RoomMessage } from "@crew/protocol";

/** 打开房间时取最新的条数，以及每次“加载更早的消息”的条数。 */
export const MESSAGE_PAGE = 100;

/**
 * 把新取到的一段消息合并进缓存：按序号去重，从旧到新排列。
 * 增量拉取与加载更早的消息可能交错返回，也可能与缓存重叠，所以不假设顺序与边界。
 */
export function mergeMessages(current: readonly RoomMessage[] | undefined, incoming: readonly RoomMessage[]) {
  const bySeq = new Map<number, RoomMessage>();
  for (const message of current ?? []) bySeq.set(message.seq, message);
  for (const message of incoming) bySeq.set(message.seq, message);
  return [...bySeq.values()].sort((a, b) => a.seq - b.seq);
}

/** 房间内序号从 1 开始连续，缓存里最早一条的序号大于 1 就还有更早的消息。 */
export function hasOlder(messages: readonly RoomMessage[]): boolean {
  return (messages[0]?.seq ?? 1) > 1;
}

/** 缓存里最新一条的序号；没有消息时为 0。 */
export function newestSeq(messages: readonly RoomMessage[]): number {
  return messages.at(-1)?.seq ?? 0;
}
