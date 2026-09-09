// The arithmetic behind `DateField`'s calendar, in a `.ts` of its own for the
// reason `weekstart.ts` gives: `tsconfig.test.json` compiles no `.svelte`, so
// anything a spec should test directly cannot live in the component.
//
// Every function here is pure and works in **civil dates** — `yyyy-mm-dd`
// strings and their parts — never instants. A calendar grid that went through
// `Date` arithmetic in the browser's zone would land a day out for anyone east
// or west of it, which is the shape of bug #44 and the reason `EventFormValue`
// carries dates as strings in the first place.

import { type WeekStartDay } from './weekstart';

/** As `Date.prototype.getDay()` numbers them: Sunday is 0. */
const JS_DAY: Record<WeekStartDay, number> = { sunday: 0, monday: 1, saturday: 6 };

export type Ymd = { y: number; m: number; d: number };

/** `yyyy-mm-dd` → its parts, or `null` for anything that is not one.
 *
 *  Strict on purpose: the field accepts typing, and a half-typed `2026-9` must
 *  read as "no date yet" rather than as September of some year. Round-trips
 *  through `Date.UTC` so `2026-02-31` is rejected rather than silently
 *  becoming the 3rd of March. */
export function parseYmd(text: string): Ymd | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(text.trim());
  if (!m) return null;
  const [y, mo, d] = [Number(m[1]), Number(m[2]), Number(m[3])];
  const probe = new Date(Date.UTC(y, mo - 1, d));
  if (probe.getUTCFullYear() !== y || probe.getUTCMonth() !== mo - 1 || probe.getUTCDate() !== d) {
    return null;
  }
  return { y, m: mo, d };
}

/** The parts back to `yyyy-mm-dd`, zero-padded. */
export const formatYmd = ({ y, m, d }: Ymd): string =>
  `${String(y).padStart(4, '0')}-${String(m).padStart(2, '0')}-${String(d).padStart(2, '0')}`;

/** Days in a month, `m` being 1-12. Day 0 of the next month is the last of
 *  this one, which is also how February gets its leap years right for free. */
export const daysInMonth = (y: number, m: number): number =>
  new Date(Date.UTC(y, m, 0)).getUTCDate();

/** `n` months from `{y, m}`, clamping the day to the shortest of the two
 *  months — stepping from the 31st into a 30-day month lands on the 30th
 *  rather than skipping a month, which is what every calendar does. */
export function addMonths({ y, m, d }: Ymd, n: number): Ymd {
  const total = y * 12 + (m - 1) + n;
  const ny = Math.floor(total / 12);
  const nm = (total % 12) + 1;
  return { y: ny, m: nm, d: Math.min(d, daysInMonth(ny, nm)) };
}

/** `n` days from a date, across month and year ends. */
export function addDays(date: Ymd, n: number): Ymd {
  const t = new Date(Date.UTC(date.y, date.m - 1, date.d + n));
  return { y: t.getUTCFullYear(), m: t.getUTCMonth() + 1, d: t.getUTCDate() };
}

/** The weekday column a date falls in, 0-6, under `start`. */
export function columnOf(date: Ymd, start: WeekStartDay): number {
  const js = new Date(Date.UTC(date.y, date.m - 1, date.d)).getUTCDay();
  // `+ 7` before the modulo for `startOfWeek`'s reason: the difference is
  // negative for most of the week under a Saturday start, and JavaScript's `%`
  // keeps its left operand's sign.
  return (js - JS_DAY[start] + 7) % 7;
}

/** The weekday headings, starting on `start`, in the browser's language.
 *
 *  Formatted from a known week rather than a hardcoded list: 2026-03-01 is a
 *  Sunday, so offsetting from it names each column without this module owning
 *  any language's day names. */
export function weekdayNames(start: WeekStartDay, locale?: string): string[] {
  const fmt = new Intl.DateTimeFormat(locale, { weekday: 'short', timeZone: 'UTC' });
  return Array.from({ length: 7 }, (_, i) =>
    fmt.format(new Date(Date.UTC(2026, 2, 1 + ((JS_DAY[start] + i) % 7)))));
}

/** The grid for the month containing `cursor`: whole weeks, so every row has
 *  seven cells, with the days either side marked as belonging to a neighbour.
 *
 *  Six rows always, not "as many as this month needs". A grid that changed
 *  height between months would move the buttons under a pointer that is
 *  already travelling towards one. */
export function monthGrid(cursor: Ymd, start: WeekStartDay): { date: Ymd; inMonth: boolean }[] {
  const first: Ymd = { y: cursor.y, m: cursor.m, d: 1 };
  const lead = columnOf(first, start);
  return Array.from({ length: 42 }, (_, i) => {
    const date = addDays(first, i - lead);
    return { date, inMonth: date.m === cursor.m && date.y === cursor.y };
  });
}
