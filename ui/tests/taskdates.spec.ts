import { test, expect } from '@playwright/test';
import {
  dueFromInputs, dueLabel, isOverdue, quickDue, timeInputValue, whenOf,
} from '../src/lib/taskdates';
import type { Task } from '../src/lib/tasks';

/**
 * The date rules behind the tasks sidebar, as a table of inputs to outputs.
 * Read back off a rendered row these would be untellable from each other:
 * "tomorrow" and "in one day" agree on most clocks and disagree on the ones
 * that matter.
 */
const task = (over: Partial<Task> = {}): Task => ({
  id: 1, calendarId: 1, summary: 'x', notes: null, dueMs: null, dueAllDay: true,
  completed: false, calendar: 'Work', color: null, priority: 0, canWrite: true, ...over,
});

// A Monday, mid-morning: 2026-09-07 10:00 local.
const MON = new Date(2026, 8, 7, 10, 0).getTime();
const DAY = 86_400_000;
const at = (days: number, hour = 9) =>
  new Date(2026, 8, 7 + days, hour, 0).getTime();

test.describe('when a task is due', () => {
  test('groups by the day, not by the hour', () => {
    expect(whenOf(task(), MON)).toBe('none');
    expect(whenOf(task({ dueMs: at(-1) }), MON)).toBe('overdue');
    // Earlier today is still today: a task due at 09:00 is not overdue at 10.
    expect(whenOf(task({ dueMs: at(0, 9) }), MON)).toBe('today');
    expect(whenOf(task({ dueMs: at(0, 23) }), MON)).toBe('today');
    expect(whenOf(task({ dueMs: at(1) }), MON)).toBe('tomorrow');
    expect(whenOf(task({ dueMs: at(4) }), MON)).toBe('week');
    // Sunday closes the week; the Monday after is Later.
    expect(whenOf(task({ dueMs: at(6) }), MON)).toBe('week');
    expect(whenOf(task({ dueMs: at(7) }), MON)).toBe('later');
  });

  /** A finished task never shouts in red, whatever its date: the overdue
   *  group is what still needs doing. */
  test('a completed task is never overdue', () => {
    const done = task({ dueMs: at(-3), completed: true });
    expect(whenOf(done, MON)).not.toBe('overdue');
    expect(isOverdue(done, MON)).toBe(false);
    expect(isOverdue(task({ dueMs: at(-3) }), MON)).toBe(true);
  });

  test('the row label says the day, and the time only when there is one', () => {
    expect(dueLabel(task({ dueMs: at(0) }), MON)).toBe('Today');
    expect(dueLabel(task({ dueMs: at(1) }), MON)).toBe('Tomorrow');
    expect(dueLabel(task({ dueMs: at(3) }), MON)).toBe('Thu 10');
    expect(dueLabel(task(), MON)).toBe('');
    // The hour follows the app's clock setting, not the locale's.
    const timed = task({ dueMs: new Date(2026, 8, 7, 18, 0).getTime(), dueAllDay: false });
    expect(dueLabel(timed, MON)).toBe('18:00');
    expect(dueLabel(timed, MON, '12h')).toBe('6:00 PM');
    const thu = task({ dueMs: new Date(2026, 8, 10, 18, 0).getTime(), dueAllDay: false });
    expect(dueLabel(thu, MON)).toBe('Thu 10 18:00');
  });
});

test.describe('setting a due date', () => {
  test('the quick answers land at the start of their day', () => {
    expect(quickDue('today', MON)).toBe(new Date(2026, 8, 7).getTime());
    expect(quickDue('tomorrow', MON)).toBe(new Date(2026, 8, 8).getTime());
    // Next week is the coming Monday, which is what people mean by it.
    expect(quickDue('nextWeek', MON)).toBe(new Date(2026, 8, 14).getTime());
    // From a Friday it is still the next Monday, not seven days on.
    expect(quickDue('nextWeek', MON + 4 * DAY)).toBe(new Date(2026, 8, 14).getTime());
  });

  /** A time with no day is not a due date, and keeping the old day would be
   *  the app deciding something the user did not. */
  test('an empty date clears the due date whatever the time says', () => {
    expect(dueFromInputs('', '18:00')).toEqual({ ms: null, allDay: true });
    expect(dueFromInputs('', '')).toEqual({ ms: null, allDay: true });
  });

  test('a date alone is all-day; a date and a time is an instant', () => {
    expect(dueFromInputs('2026-09-10', '')).toEqual({
      ms: new Date(2026, 8, 10).getTime(), allDay: true,
    });
    expect(dueFromInputs('2026-09-10', '18:30')).toEqual({
      ms: new Date(2026, 8, 10, 18, 30).getTime(), allDay: false,
    });
  });

  test('the time field is empty for an all-day task and filled for a timed one', () => {
    expect(timeInputValue(task({ dueMs: at(0), dueAllDay: true }))).toBe('');
    expect(timeInputValue(task({ dueMs: new Date(2026, 8, 7, 18, 5).getTime(), dueAllDay: false })))
      .toBe('18:05');
    expect(timeInputValue(task())).toBe('');
  });
});
