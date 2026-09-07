import { invoke } from '@tauri-apps/api/core';

/** One task, as `tasks::list_tasks` shapes it. */
export type Task = {
  id: number;
  calendarId: number;
  summary: string;
  notes: string | null;
  dueMs: number | null;
  dueAllDay: boolean;
  completed: boolean;
  calendar: string;
  color: string | null;
  priority: number;
  /** False on read-only lists and in demo mode — the checkbox renders
   *  disabled rather than pretending. */
  canWrite: boolean;
};

/** One list the quick-add can land on. */
export type TaskList = {
  calendarId: number;
  name: string;
  color: string | null;
};

export const listTasks = () => invoke<Task[]>('list_tasks');
export const taskLists = () => invoke<TaskList[]>('task_lists');

/** Completes (or reopens) a task — the server first, then the store, which is
 *  why the fresh list comes back from the same call. */
export const setTaskCompleted = (id: number, on: boolean) =>
  invoke<Task[]>('set_task_completed', { id, on });

export const createTask = (calendarId: number, summary: string, dueMs: number | null) =>
  invoke<Task[]>('create_task', { calendarId, summary, dueMs });

export const deleteTask = (id: number) => invoke<Task[]>('delete_task_cmd', { id });

/** Edits a task whole: title, due date and note in one write.
 *
 *  Every field is the complete answer rather than a change to apply, so a
 *  `null` due date clears it. `dueAllDay` is the difference between "by
 *  Thursday" and "by Thursday at 18:00", which is the user's distinction
 *  and not a storage detail — the backend spells the two differently on the
 *  wire. */
export const updateTask = (
  id: number,
  summary: string,
  dueMs: number | null,
  dueAllDay: boolean,
  notes: string | null,
) => invoke<Task[]>('update_task', { id, summary, dueMs, dueAllDay, notes });

/** Connects an iCloud or generic CalDAV account. Resolves to the account's
 *  display email once discovery has accepted the credentials. */
export const connectCaldav = (args: {
  kind: 'icloud' | 'caldav';
  serverUrl?: string;
  email: string;
  username?: string;
  password: string;
}) =>
  invoke<string>('connect_caldav', {
    kind: args.kind,
    serverUrl: args.serverUrl ?? null,
    email: args.email,
    username: args.username ?? null,
    password: args.password,
  });
