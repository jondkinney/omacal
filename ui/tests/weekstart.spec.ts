import { test, expect } from '@playwright/test';
import { isWeekendColumn, rotate, startOfWeek } from '../src/lib/weekstart';

// The browser half of the week-start arithmetic, as a table. The Rust suite
// owns the grids these feed; what can still be wrong here is the modulo.

/** A local-midnight date, built the way the app builds them. */
const on = (y: number, m: number, d: number) => new Date(y, m - 1, d);
const dayName = (ms: number) =>
  new Date(ms).toLocaleDateString('en-GB', { weekday: 'long' });

test.describe('startOfWeek', () => {
  // Saturday 1 August 2026 — the day that separates all three starts, and the
  // one the Rust anchor tests use for the same reason.
  test('a Saturday belongs to a different week under each start', () => {
    const sat = on(2026, 8, 1);
    expect(dayName(startOfWeek(sat, 'monday'))).toBe('Monday');
    expect(dayName(startOfWeek(sat, 'sunday'))).toBe('Sunday');
    expect(dayName(startOfWeek(sat, 'saturday'))).toBe('Saturday');

    // Saturday *is* its own week's first day under a Saturday start.
    expect(startOfWeek(sat, 'saturday')).toBe(sat.getTime());
    // And the counter-intuitive ordering the Rust test also pins: from a
    // Saturday, the Sunday start reaches back furthest.
    expect(startOfWeek(sat, 'sunday')).toBeLessThan(startOfWeek(sat, 'monday'));
  });

  /** **The sign bug this function is written to avoid.** `getDay() - 6` is
   *  negative from Sunday through Friday, and JavaScript's `%` keeps the sign
   *  of its left operand, so a missing `+ 7` lands the week days ahead
   *  instead of behind — the failure looks like a calendar showing next week. */
  test('every day of a week resolves to that week, never a later one', () => {
    for (const start of ['monday', 'sunday', 'saturday'] as const) {
      for (let d = 1; d <= 31; d++) {
        const day = on(2026, 8, d);
        const ws = startOfWeek(day, start);
        expect(ws, `${start} ${d} Aug went forwards`).toBeLessThanOrEqual(day.getTime());
        expect(day.getTime() - ws, `${start} ${d} Aug is more than a week out`)
          .toBeLessThan(7 * 24 * 3_600_000);
      }
    }
  });
});

test.describe('rotate', () => {
  const MONDAY_FIRST = ['MON', 'TUE', 'WED', 'THU', 'FRI', 'SAT', 'SUN'] as const;

  test('the labels start on the chosen day and keep their order', () => {
    expect(rotate(MONDAY_FIRST, 'monday')).toEqual([...MONDAY_FIRST]);
    expect(rotate(MONDAY_FIRST, 'sunday'))
      .toEqual(['SUN', 'MON', 'TUE', 'WED', 'THU', 'FRI', 'SAT']);
    expect(rotate(MONDAY_FIRST, 'saturday'))
      .toEqual(['SAT', 'SUN', 'MON', 'TUE', 'WED', 'THU', 'FRI']);
  });

  test('nothing is dropped or duplicated', () => {
    for (const start of ['monday', 'sunday', 'saturday'] as const) {
      const out = rotate(MONDAY_FIRST, start);
      expect(out).toHaveLength(7);
      expect(new Set(out).size).toBe(7);
    }
  });
});

test.describe('isWeekendColumn', () => {
  // Mirrors `the_ribbons_weekend_stripes_stay_straight_under_every_start`.
  // If these two tables ever disagree, the ribbon shades the wrong cells.
  test('the weekend sits where the chosen week puts it', () => {
    const cols = (start: 'monday' | 'sunday' | 'saturday') =>
      Array.from({ length: 28 }, (_, c) => c).filter((c) => isWeekendColumn(c, start));

    expect(cols('monday')).toEqual([5, 6, 12, 13, 19, 20, 26, 27]);
    expect(cols('sunday')).toEqual([0, 6, 7, 13, 14, 20, 21, 27]);
    expect(cols('saturday')).toEqual([0, 1, 7, 8, 14, 15, 21, 22]);
  });
});
