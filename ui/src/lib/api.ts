import { invoke } from '@tauri-apps/api/core';

export type UiEvent = {
  id: number; title: string; location: string | null;
  start_ms: number; end_ms: number; color: string;
  response: 'accepted' | 'needsAction' | 'tentative' | 'declined';
  is_all_day: boolean;
};
export type Placed = { idx: number; column: number; columns: number; top: number; height: number };
export type Lane = {
  idx: number; lane: number; start_col: number; end_col: number;
  cont_left: boolean; cont_right: boolean;
};
/** `end_ms` is midnight on the next day, so a 23- or 25-hour DST day reports
 *  its true span rather than a nominal 24 hours. */
export type DayColumn = {
  start_ms: number; end_ms: number; events: UiEvent[]; placed: Placed[];
};
export type WeekPayload = {
  days: DayColumn[]; all_day: Lane[]; all_day_events: UiEvent[]; overflow: number[];
};

/** One day of a month grid. `in_month` is false for the leading/trailing days
 *  that belong to a neighbouring month — drawn dimmed, not hidden, so the
 *  grid stays rectangular. `timed` is complete and sorted by start; the UI
 *  decides how many lines fit and computes its own `+N more`. */
export type MonthCell = { start_ms: number; end_ms: number; in_month: boolean; timed: UiEvent[] };
/** One week-row of a month grid: seven cells plus the multi-day/all-day bars
 *  spanning them, already lane-packed and row-clipped (`bars: Lane[]`, each
 *  entry one placed segment — not a container of segments). */
export type MonthRow = {
  cells: MonthCell[]; bars: Lane[]; bar_events: UiEvent[]; bar_overflow: number[];
};
/** `rows` is always 6, even when the month fits in five, so the grid never
 *  changes height as you page through the year. */
export type MonthPayload = { rows: MonthRow[]; year: number; month: number };

/** Midnight local on the Monday of the week containing `d`. */
export function weekStart(d: Date): number {
  const m = new Date(d);
  m.setHours(0, 0, 0, 0);
  m.setDate(m.getDate() - ((m.getDay() + 6) % 7));
  return m.getTime();
}

export const getWeek = (weekStartMs: number) =>
  invoke<WeekPayload>('get_week', { weekStartMs });

export const getDay = (dayStartMs: number) =>
  invoke<WeekPayload>('get_day', { dayStartMs });

export const getMonth = (year: number, month: number) =>
  invoke<MonthPayload>('get_month', { year, month });

/** One day of the year grid. `has_all_day` is set only by an all-day event —
 *  a timed meeting does not dot this view. `unsynced` marks a day outside
 *  the window the app actually keeps fetched (`synced_window` in Rust);
 *  drawn distinctly from an in-window day with nothing on it, since absence
 *  of a dot must never read as "free". */
export type YearDay = { start_ms: number; day: number; has_all_day: boolean; unsynced: boolean };
/** One month of the year grid: `lead_blanks` empty cells (Monday-first)
 *  before day 1, so every month's weekday columns line up. */
export type YearMonth = { month: number; lead_blanks: number; days: YearDay[] };
export type YearPayload = { year: number; months: YearMonth[] };

export const getYear = (year: number) =>
  invoke<YearPayload>('get_year', { year });
