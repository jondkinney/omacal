// A task as the week grid draws it, and how a week's worth are found.
//
// The grid takes chips rather than `Task`s so it never has to know what a
// task list is, only what belongs on Thursday — the same bargain the
// forecast makes with `weather.ts`.

import { dateKey } from './weather';
import type { Task } from './tasks';

export type TaskChip = {
  id: number;
  summary: string;
  color: string | null;
  completed: boolean;
  overdue: boolean;
  canWrite: boolean;
};

/**
 * The tasks the grid should draw, by the ISO date they belong on.
 *
 * Two rules, both about not lying:
 *
 * - An **overdue** task is drawn on today, not on the day it was due. Its
 *   date has passed and the column for it may not even be on screen; a task
 *   that quietly leaves the week when the week moves is one nobody does.
 *   It is marked, so the day it lands on is not read as its due date.
 * - A **completed** task is left out. The row is what still needs doing,
 *   and the sidebar's Done section is where a finished one goes.
 */
export function taskChips(
  tasks: Task[],
  nowMs: number,
  lists: { calendarId: number; color: string | null }[] = [],
): Map<string, TaskChip[]> {
  const today = dateKey(nowMs);
  const out = new Map<string, TaskChip[]>();
  for (const t of tasks) {
    if (t.completed || t.dueMs === null) continue;
    const overdue = dateKey(t.dueMs) < today;
    const key = overdue ? today : dateKey(t.dueMs);
    const chip: TaskChip = {
      id: t.id,
      summary: t.summary,
      color: t.color ?? lists.find((l) => l.calendarId === t.calendarId)?.color ?? null,
      completed: false,
      overdue,
      canWrite: t.canWrite,
    };
    const at = out.get(key);
    if (at) at.push(chip);
    else out.set(key, [chip]);
  }
  return out;
}
