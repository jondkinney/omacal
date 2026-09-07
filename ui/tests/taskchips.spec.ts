import { test, expect } from '@playwright/test';
import { taskChips } from '../src/lib/taskchips';
import type { Task } from '../src/lib/tasks';

/**
 * Which day a task is drawn on. Two rules, both about not lying, and both
 * invisible from a rendered row: an overdue task moves to today rather than
 * off the screen, and a finished one is not drawn at all.
 */
const task = (over: Partial<Task> = {}): Task => ({
  id: 1, calendarId: 1, summary: 'x', notes: null, dueMs: null, dueAllDay: true,
  completed: false, calendar: 'Work', color: '#2dd4bf', priority: 0, canWrite: true, ...over,
});
const NOW = new Date(2026, 8, 9, 10, 0).getTime(); // Wed 9 Sep 2026
const on = (day: number, hour = 9) => new Date(2026, 8, day, hour, 0).getTime();

test('a task is drawn on the day it is due', () => {
  const map = taskChips([task({ id: 1, dueMs: on(11) })], NOW);
  expect([...map.keys()]).toEqual(['2026-09-11']);
  expect(map.get('2026-09-11')![0]).toMatchObject({ id: 1, overdue: false, summary: 'x' });
});

/** Its own day may not be on screen, and a task that leaves the week when
 *  the week moves is one nobody does. It is marked, so the day it lands on
 *  is not read as its due date. */
test('an overdue task is drawn on today, and marked', () => {
  const map = taskChips([task({ id: 2, dueMs: on(4) })], NOW);
  expect([...map.keys()]).toEqual(['2026-09-09']);
  expect(map.get('2026-09-09')![0].overdue).toBe(true);
});

test('a completed task and an undated one are not drawn at all', () => {
  const map = taskChips([
    task({ id: 3, dueMs: on(11), completed: true }),
    task({ id: 4, dueMs: null }),
    // Completed *and* overdue: still not drawn, and not piled onto today.
    task({ id: 5, dueMs: on(4), completed: true }),
  ], NOW);
  expect(map.size).toBe(0);
});

test('several tasks on one day keep their order', () => {
  const map = taskChips([
    task({ id: 6, dueMs: on(11), summary: 'first' }),
    task({ id: 7, dueMs: on(11, 14), summary: 'second' }),
  ], NOW);
  expect(map.get('2026-09-11')!.map((c) => c.summary)).toEqual(['first', 'second']);
});

/** A task's own colour wins; without one it takes its list's, and without
 *  that the grid falls back rather than drawing nothing. */
test('a chip takes its colour from the task, else from its list', () => {
  const lists = [{ calendarId: 9, color: '#7aa2f7' }];
  expect(taskChips([task({ dueMs: on(11) })], NOW, lists).get('2026-09-11')![0].color).toBe('#2dd4bf');
  expect(taskChips([task({ dueMs: on(11), calendarId: 9, color: null })], NOW, lists)
    .get('2026-09-11')![0].color).toBe('#7aa2f7');
  expect(taskChips([task({ dueMs: on(11), calendarId: 1, color: null })], NOW, lists)
    .get('2026-09-11')![0].color).toBe(null);
});
