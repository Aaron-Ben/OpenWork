import { TASK_TRANSITIONS, type TaskStatus, type TaskView } from "@crew/protocol";

// 任务在界面上的排列：列表按“要你看的、正在做的、还没开始的”排，完成与关闭折叠在最后；看板固定五列。

/** 列表里分组的顺序：待审要用户确认，排最前。 */
export const LIST_ORDER: readonly TaskStatus[] = ["in_review", "in_progress", "todo"];
/** 列表里折叠在最后的状态。 */
export const FINISHED: readonly TaskStatus[] = ["done", "closed"];
/** 看板的列。 */
export const BOARD_ORDER: readonly TaskStatus[] = ["todo", "in_progress", "in_review", "done", "closed"];

/** 按状态分组，每组按编号排列。 */
export function groupByStatus(tasks: readonly TaskView[]): Map<TaskStatus, TaskView[]> {
  const groups = new Map<TaskStatus, TaskView[]>();
  for (const task of [...tasks].sort((a, b) => a.number - b.number)) {
    groups.set(task.status, [...(groups.get(task.status) ?? []), task]);
  }
  return groups;
}

/** 还没完成的任务数：聊天头部“任务”按钮上的数字。 */
export function openCount(tasks: readonly TaskView[]): number {
  return tasks.filter((task) => !FINISHED.includes(task.status)).length;
}

/**
 * 状态下拉菜单里能选的状态：流转表允许的，去掉要有负责人却没有的（进行中与待审）。
 */
export function nextStatuses(task: Pick<TaskView, "status" | "assignee">): TaskStatus[] {
  return TASK_TRANSITIONS[task.status].filter(
    (status) => task.assignee !== null || (status !== "in_progress" && status !== "in_review"),
  );
}
