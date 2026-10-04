export interface ScrollMetrics {
  scrollTop: number;
  scrollHeight: number;
  clientHeight: number;
}

/** 距离底部多少像素以内算作“停在底部”。 */
const NEAR_BOTTOM_PX = 80;

/** 停在底部附近时，新消息到来自动滚到底；用户往上翻看历史时不打断。 */
export function isNearBottom({ scrollTop, scrollHeight, clientHeight }: ScrollMetrics): boolean {
  return scrollHeight - scrollTop - clientHeight <= NEAR_BOTTOM_PX;
}
