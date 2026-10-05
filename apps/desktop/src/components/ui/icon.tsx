import type { ReactNode } from "react";
import { cn } from "../../lib/cn";

// 线条小图标：都画在 16×16 的画布上，圆头圆角。笔画不随图标大小缩放，屏幕上一律 1.5px 粗，
// 13px 的通知图标与 15px 的按钮图标看起来一样粗。尺寸与颜色由调用方的 className 决定（颜色取 currentColor）。

const paths = {
  // 栏头与侧栏的按钮
  back: <path d="M10 3 5 8l5 5" />,
  expand: <path d="M9.5 2.5h4v4M6.5 13.5h-4v-4M13.5 2.5 9 7M2.5 13.5 7 9" />,
  collapse: <path d="M13.5 6.5h-4v-4M2.5 9.5h4v4M9.5 6.5 14 2M6.5 9.5 2 14" />,
  close: <path d="M4 4l8 8M12 4l-8 8" />,
  sidebar: (
    <>
      <rect x="2" y="3" width="12" height="10" rx="2" />
      <path d="M6 3v10" />
    </>
  ),
  // 通知
  clipboard: (
    <>
      <rect x="3" y="2.5" width="10" height="11.5" rx="2" />
      <path d="M6 2.5h4v2H6zM6 8h4M6 10.5h3" />
    </>
  ),
  play: <path d="M5 3v10l8-5z" />,
  eye: (
    <>
      <path d="M2.5 8s2-4 5.5-4 5.5 4 5.5 4-2 4-5.5 4-5.5-4-5.5-4z" />
      <circle cx="8" cy="8" r="1.6" />
    </>
  ),
  assign: (
    <>
      <circle cx="6" cy="5.5" r="2.5" />
      <path d="M2 13.5c.6-2.3 2.1-3.5 4-3.5s3.4 1.2 4 3.5M11 6h4M13 4v4" />
    </>
  ),
  sendBack: <path d="M6 4 3 7l3 3M3 7h6.5a3.5 3.5 0 0 1 0 7H8" />,
  check: <path d="m3.5 8.5 3 3 6-7" />,
  closed: (
    <>
      <circle cx="8" cy="8" r="5.5" />
      <path d="M4.2 11.8l7.6-7.6" />
    </>
  ),
  alarm: (
    <>
      <circle cx="8" cy="9" r="5" />
      <path d="M8 6.5V9l1.8 1.2M2.5 3.5 4.5 2M13.5 3.5 11.5 2" />
    </>
  ),
  dot: <circle cx="8" cy="8" r="2" />,
} satisfies Record<string, ReactNode>;

export type IconName = keyof typeof paths;

export function Icon({ name, className }: { name: IconName; className?: string }) {
  return (
    <svg
      viewBox="0 0 16 16"
      className={cn("size-4 flex-none fill-none stroke-current [&_*]:[vector-effect:non-scaling-stroke]", className)}
      strokeWidth={1.5}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {paths[name]}
    </svg>
  );
}
