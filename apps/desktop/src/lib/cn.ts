import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

/** 合并 class：后写的 Tailwind 工具类覆盖先写的同类工具类。 */
export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs));
}
