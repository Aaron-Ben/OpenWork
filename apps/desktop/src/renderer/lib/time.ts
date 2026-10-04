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
