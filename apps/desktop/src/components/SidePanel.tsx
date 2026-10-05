import type { ReactNode } from "react";
import { cn } from "../lib/cn";

/**
 * 右栏：运行记录与讨论串共用的外框，同时只显示一个。栏头右上角放大或还原整栏；放大后还可以收起左侧会话栏（专注），
 * 只留一条窄的聊天。设计稿是 out/mockups/step5-threads-tasks.html。
 */
export function SidePanel({
  title,
  subtitle,
  expanded,
  focus,
  onExpandedChange,
  onFocusChange,
  onBack,
  onClose,
  children,
}: {
  title: string;
  subtitle?: string;
  expanded: boolean;
  focus: boolean;
  onExpandedChange(expanded: boolean): void;
  onFocusChange(focus: boolean): void;
  /** 有值时在标题前显示返回按钮，例如从一个讨论串回到讨论串列表。 */
  onBack?(): void;
  onClose(): void;
  children: ReactNode;
}) {
  return (
    <aside
      className={cn("flex min-w-0 flex-col border-l border-line bg-bg", expanded ? "flex-1" : "w-[420px] flex-none")}
    >
      <header className="drag flex h-[52px] flex-none items-center gap-2 border-b border-line pr-2.5 pl-3.5">
        {onBack && (
          <IconButton label="返回" onClick={onBack}>
            <path d="M10 3 5 8l5 5" />
          </IconButton>
        )}
        <div className={cn("min-w-0", !onBack && "pl-1")}>
          <b className="block truncate text-sm">{title}</b>
          {subtitle && <span className="-mt-0.5 block truncate text-[11.5px] text-faint">{subtitle}</span>}
        </div>
        <div className="ml-auto flex flex-none gap-0.5">
          {expanded && (
            <IconButton
              label={focus ? "展开会话栏" : "收起会话栏"}
              active={focus}
              onClick={() => onFocusChange(!focus)}
            >
              <rect x="2.5" y="3" width="11" height="10" rx="2" />
              <path d="M6.5 3v10" />
            </IconButton>
          )}
          <IconButton label={expanded ? "还原" : "放大"} onClick={() => onExpandedChange(!expanded)}>
            {expanded ? (
              <path d="M13.5 6.5h-4v-4M2.5 9.5h4v4M9.5 6.5 14 2M6.5 9.5 2 14" />
            ) : (
              <path d="M9.5 2.5h4v4M6.5 13.5h-4v-4M13.5 2.5 9 7M2.5 13.5 7 9" />
            )}
          </IconButton>
          <IconButton label="关闭" onClick={onClose}>
            <path d="M4 4l8 8M12 4l-8 8" />
          </IconButton>
        </div>
      </header>
      {children}
    </aside>
  );
}

function IconButton({
  label,
  active = false,
  onClick,
  children,
}: {
  label: string;
  active?: boolean;
  onClick(): void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      className={cn(
        "grid size-7 place-items-center rounded-md hover:bg-hover hover:text-text",
        active ? "bg-hover text-text" : "text-faint",
      )}
    >
      <svg
        viewBox="0 0 16 16"
        className="size-[15px] fill-none stroke-current"
        strokeWidth={1.6}
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        {children}
      </svg>
    </button>
  );
}
