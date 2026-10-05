import type { ReactNode } from "react";
import { cn } from "../lib/cn";
import { Icon, type IconName } from "./ui/icon";

/**
 * 右栏：运行记录、讨论串与任务共用的一张卡片，同时只显示一个。栏头右上角放大或还原：放大后右栏占去聊天区与右栏两栏，
 * 聊天区让出来，聊天顶栏的视图切换（`tools`）搬到栏头。侧栏收起时，放大的右栏栏头最左边是“显示侧栏”（`leading`）。
 * 设计稿是 out/mockups/step5-layout.html（布局）与 step5-cards.html（卡片）。
 */
export function SidePanel({
  title,
  subtitle,
  expanded,
  tools,
  leading,
  onExpandedChange,
  onBack,
  onClose,
  children,
}: {
  title: string;
  subtitle?: string;
  expanded: boolean;
  /** 放大时显示在栏头的视图切换。 */
  tools: ReactNode;
  /** 放大并且侧栏收起时显示在栏头最左边，给系统的红黄绿按钮留出位置。 */
  leading?: ReactNode;
  onExpandedChange(expanded: boolean): void;
  /** 有值时在标题前显示返回按钮，例如从一个讨论串回到讨论串列表。 */
  onBack?(): void;
  onClose(): void;
  children: ReactNode;
}) {
  const lead = expanded ? leading : undefined;
  return (
    // 外层留白，里面是一张浮在页面上的卡片：圆角、阴影，没有分隔线。
    <div className={cn("flex min-w-0 py-2.5 pr-2.5", expanded ? "flex-1 pl-2.5" : "w-[430px] flex-none")}>
      <aside className="flex min-w-0 flex-1 flex-col overflow-hidden rounded-2xl border border-line bg-raised shadow-lift">
        <header className={cn("drag flex h-[54px] flex-none items-center gap-2 pr-2", lead ? "pl-[74px]" : "pl-3.5")}>
          {lead}
          {onBack && <IconButton label="返回" icon="back" onClick={onBack} />}
          <div className={cn("min-w-0", !onBack && !lead && "pl-1")}>
            <b className="block truncate text-sm">{title}</b>
            {subtitle && <span className="-mt-0.5 block truncate text-[11.5px] text-faint">{subtitle}</span>}
          </div>
          <div className="ml-auto flex flex-none items-center gap-0.5">
            {expanded && <div className="mr-2">{tools}</div>}
            <IconButton
              label={expanded ? "还原" : "放大"}
              icon={expanded ? "collapse" : "expand"}
              onClick={() => onExpandedChange(!expanded)}
            />
            <IconButton label="关闭" icon="close" onClick={onClose} />
          </div>
        </header>
        {children}
      </aside>
    </div>
  );
}

/** 显示或隐藏侧栏的按钮：侧栏顶部是“隐藏侧栏”，侧栏收起后聊天顶栏或放大的右栏栏头是“显示侧栏”。快捷键 ⌘\。 */
export function SidebarToggle({ hidden, onClick }: { hidden: boolean; onClick(): void }) {
  return <IconButton label={hidden ? "显示侧栏（⌘\\）" : "隐藏侧栏（⌘\\）"} icon="sidebar" onClick={onClick} />;
}

function IconButton({ label, icon, onClick }: { label: string; icon: IconName; onClick(): void }) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      className="grid size-7 place-items-center rounded-md text-faint hover:bg-hover hover:text-text"
    >
      <Icon name={icon} className="size-[15px]" />
    </button>
  );
}
