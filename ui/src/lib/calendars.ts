import { invoke } from '@tauri-apps/api/core';

export type Calendar = {
  id: number;
  account_id: number;
  account_email: string;
  summary: string;
  color_hex: string | null;
  /** Drawn in the grid. */
  selected: boolean;
  /** Fetched from Google at all. */
  sync_enabled: boolean;
  is_primary: boolean;
  /** Google's own word for what this account may do here: `owner`, `writer`,
   *  `reader`, `freeBusyReader`. Only the first two can be written to — see
   *  `writableCalendars` below, the one place that decides it. */
  access_role: string;
};

/** The two roles a create can land on. Google's `calendarList` reports two more
 *  — `reader` and `freeBusyReader` — and a subscribed holiday calendar is a
 *  `reader`, so a list that is not filtered offers Save buttons the backend can
 *  only refuse (`create_impl`'s own `can_edit` check, against the same column). */
export const writableCalendars = (cals: Calendar[]) =>
  cals.filter((c) => c.access_role === 'owner' || c.access_role === 'writer');

export const getCalendars = () => invoke<Calendar[]>('get_calendars');
export const setCalendarSelected = (id: number, on: boolean) =>
  invoke<void>('set_calendar_selected', { id, on });
/** Resolves to the number of local events the removal deleted. */
export const setCalendarSync = (id: number, on: boolean) =>
  invoke<number>('set_calendar_sync', { id, on });

/** Calendars grouped by account, preserving the order the backend returned. */
export function byAccount(cals: Calendar[]): Array<[string, Calendar[]]> {
  const groups = new Map<string, Calendar[]>();
  for (const c of cals) {
    const g = groups.get(c.account_email) ?? [];
    g.push(c);
    groups.set(c.account_email, g);
  }
  return [...groups.entries()];
}
