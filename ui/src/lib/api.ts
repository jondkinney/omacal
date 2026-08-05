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

/** Midnight local on the Monday of the week containing `d`. */
export function weekStart(d: Date): number {
  const m = new Date(d);
  m.setHours(0, 0, 0, 0);
  m.setDate(m.getDate() - ((m.getDay() + 6) % 7));
  return m.getTime();
}

export const getWeek = (weekStartMs: number) =>
  invoke<WeekPayload>('get_week', { weekStartMs });
