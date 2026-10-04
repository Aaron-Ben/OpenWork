const pad = (value: number) => String(value).padStart(2, "0");

/** 消息时间：今天只写时分，今年写月日，更早的写年月日。按本机时区。 */
export function formatMessageTime(iso: string, now: Date): string {
  const time = new Date(iso);
  const clock = `${pad(time.getHours())}:${pad(time.getMinutes())}`;
  if (time.toDateString() === now.toDateString()) return clock;
  const day = `${time.getMonth() + 1}月${time.getDate()}日`;
  if (time.getFullYear() === now.getFullYear()) return `${day} ${clock}`;
  return `${time.getFullYear()}年${day} ${clock}`;
}

/** 会话列表的时间：今天写时分，昨天写“昨天”，今年写月日，更早的写年月日。 */
export function formatListTime(iso: string, now: Date): string {
  const time = new Date(iso);
  if (time.toDateString() === now.toDateString()) return `${pad(time.getHours())}:${pad(time.getMinutes())}`;
  const yesterday = new Date(now);
  yesterday.setDate(now.getDate() - 1);
  if (time.toDateString() === yesterday.toDateString()) return "昨天";
  const day = `${time.getMonth() + 1}月${time.getDate()}日`;
  if (time.getFullYear() === now.getFullYear()) return day;
  return `${time.getFullYear()}/${time.getMonth() + 1}/${time.getDate()}`;
}
