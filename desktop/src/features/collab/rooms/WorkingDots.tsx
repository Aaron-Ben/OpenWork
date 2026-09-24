/** 跳动的三点：有 Agent 正在处理（collaboration-desktop.md §7.1、§7.4）。 */
export function WorkingDots() {
  return (
    <span aria-hidden="true" className="inline-flex shrink-0 items-center gap-0.5">
      {[0, 150, 300].map((delay) => (
        <span key={delay} className="size-1 animate-bounce rounded-full bg-current" style={{ animationDelay: `${delay}ms` }} />
      ))}
    </span>
  )
}
