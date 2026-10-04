// 头像：颜色、显示的字与群聊头像里成员的位置。

/** 头像底色。浅色与深色主题共用，字是白色，每种颜色都要和白字有足够的对比度。 */
export const AVATAR_COLORS = [
  "#2563eb",
  "#9333ea",
  "#0d9488",
  "#db2777",
  "#ca8a04",
  "#4f46e5",
  "#0891b2",
  "#c2410c",
] as const;

/** 由 handle 决定的颜色：同一个 Agent 在哪里都是同一种颜色。 */
export function avatarColor(key: string): string {
  let hash = 0;
  for (const char of key) hash = (hash * 31 + (char.codePointAt(0) ?? 0)) >>> 0;
  return AVATAR_COLORS[hash % AVATAR_COLORS.length] ?? AVATAR_COLORS[0];
}

/** 头像上的字：名字的第一个字符，英文字母大写。 */
export function avatarInitial(name: string): string {
  const first = [...name.trim()][0] ?? "?";
  return first.toUpperCase();
}

/** 群聊头像最多显示的成员数。 */
export const RING_MAX = 5;

export interface RingSlot {
  /** 成员头像中心的位置与直径，都是群聊头像边长的比例（0 到 1）。 */
  x: number;
  y: number;
  size: number;
}

/**
 * 群聊头像里成员的位置：最多 5 个，围成一个环，顺时针排列。
 * 只有一个成员时居中放大；成员越多，每个头像越小。头像贴着外圈内侧排列，相邻的不重叠，字不会被盖住。
 */
export function ringSlots(count: number): RingSlot[] {
  const n = Math.min(Math.max(count, 0), RING_MAX);
  if (n === 0) return [];
  if (n === 1) return [{ x: 0.5, y: 0.5, size: 0.62 }];
  const size = n <= 3 ? 0.44 : n === 4 ? 0.4 : 0.34;
  const radius = 0.5 - size / 2 - 0.01;
  return Array.from({ length: n }, (_, i) => {
    // 两个成员沿对角线放（左上、右下），比上下叠着自然；三个及以上从正上方开始。
    const start = n === 2 ? (-3 * Math.PI) / 4 : -Math.PI / 2;
    const angle = start + (2 * Math.PI * i) / n;
    return { x: 0.5 + radius * Math.cos(angle), y: 0.5 + radius * Math.sin(angle), size };
  });
}
