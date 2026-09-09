import { test, expect } from '@playwright/test';
import {
  addDays, addMonths, columnOf, daysInMonth, formatYmd, monthGrid, parseYmd, weekdayNames,
} from '../src/lib/datepicker';

test.describe('the date picker arithmetic', () => {
  test('parsing is strict, because a half-typed date is not a date', () => {
    expect(parseYmd('2026-09-06')).toEqual({ y: 2026, m: 9, d: 6 });
    expect(parseYmd('  2026-09-06 ')).toEqual({ y: 2026, m: 9, d: 6 });
    for (const bad of ['', '2026-9-6', '2026-09', '06/09/2026', 'tomorrow', '2026-09-06x']) {
      expect(parseYmd(bad), `${bad} parsed`).toBeNull();
    }
    // Not merely shaped right: a day that does not exist is refused rather
    // than rolling into the next month, which is what `Date` would do.
    expect(parseYmd('2026-02-30')).toBeNull();
    expect(parseYmd('2025-02-29'), 'not a leap year').toBeNull();
    expect(parseYmd('2024-02-29'), 'a leap year').toEqual({ y: 2024, m: 2, d: 29 });
  });

  test('formatting pads, and round-trips whatever parsing accepted', () => {
    expect(formatYmd({ y: 2026, m: 9, d: 6 })).toBe('2026-09-06');
    for (const s of ['2026-01-01', '2024-02-29', '1999-12-31']) {
      expect(formatYmd(parseYmd(s)!)).toBe(s);
    }
  });

  test('month lengths, February included', () => {
    expect(daysInMonth(2026, 2)).toBe(28);
    expect(daysInMonth(2024, 2)).toBe(29);
    expect(daysInMonth(2026, 9)).toBe(30);
    expect(daysInMonth(2026, 12)).toBe(31);
  });

  /** Stepping a month from the 31st must not skip one — the trap every
   *  hand-rolled calendar falls into. */
  test('a month step clamps the day rather than overflowing', () => {
    expect(addMonths({ y: 2026, m: 1, d: 31 }, 1)).toEqual({ y: 2026, m: 2, d: 28 });
    expect(addMonths({ y: 2024, m: 1, d: 31 }, 1)).toEqual({ y: 2024, m: 2, d: 29 });
    expect(addMonths({ y: 2026, m: 12, d: 15 }, 1)).toEqual({ y: 2027, m: 1, d: 15 });
    expect(addMonths({ y: 2026, m: 1, d: 15 }, -1)).toEqual({ y: 2025, m: 12, d: 15 });
    expect(addMonths({ y: 2026, m: 3, d: 31 }, -1)).toEqual({ y: 2026, m: 2, d: 28 });
  });

  test('a day step crosses months and years', () => {
    expect(addDays({ y: 2026, m: 1, d: 31 }, 1)).toEqual({ y: 2026, m: 2, d: 1 });
    expect(addDays({ y: 2026, m: 1, d: 1 }, -1)).toEqual({ y: 2025, m: 12, d: 31 });
    expect(addDays({ y: 2024, m: 2, d: 28 }, 1)).toEqual({ y: 2024, m: 2, d: 29 });
  });

  /** The `% 7` sign trap `startOfWeek` documents, in this module's own form:
   *  under a Saturday start most of the week is a negative difference. */
  test('the weekday column honours every start without going negative', () => {
    const sunday = { y: 2026, m: 3, d: 1 };
    expect(columnOf(sunday, 'sunday')).toBe(0);
    expect(columnOf(sunday, 'monday')).toBe(6);
    expect(columnOf(sunday, 'saturday')).toBe(1);
    for (const start of ['monday', 'sunday', 'saturday'] as const) {
      for (let i = 0; i < 14; i += 1) {
        const c = columnOf(addDays(sunday, i), start);
        expect(c, `${start} +${i}`).toBeGreaterThanOrEqual(0);
        expect(c).toBeLessThan(7);
      }
    }
  });

  test('the weekday headings begin on the chosen day', () => {
    expect(weekdayNames('monday', 'en-GB')[0]).toBe('Mon');
    expect(weekdayNames('sunday', 'en-GB')[0]).toBe('Sun');
    expect(weekdayNames('saturday', 'en-GB')[0]).toBe('Sat');
    expect(weekdayNames('monday', 'en-GB')).toHaveLength(7);
    expect(new Set(weekdayNames('monday', 'en-GB')).size, 'a repeated heading').toBe(7);
  });

  test('the grid is six whole weeks, aligned, and knows its own month', () => {
    const grid = monthGrid({ y: 2026, m: 9, d: 6 }, 'monday');
    expect(grid).toHaveLength(42);
    // Every row starts on the chosen day, which is what "aligned" means.
    for (let row = 0; row < 6; row += 1) {
      expect(columnOf(grid[row * 7].date, 'monday'), `row ${row}`).toBe(0);
    }
    // September 2026 begins on a Tuesday, so a Monday grid leads with one
    // August day and the 1st sits in the second cell.
    expect(grid[0].inMonth).toBe(false);
    expect(formatYmd(grid[1].date)).toBe('2026-09-01');
    expect(grid.filter((c) => c.inMonth)).toHaveLength(30);
    // Consecutive throughout: no gap where a month changes.
    for (let i = 1; i < grid.length; i += 1) {
      expect(formatYmd(grid[i].date)).toBe(formatYmd(addDays(grid[i - 1].date, 1)));
    }
  });

  /** Six rows always. A grid that shrank to five would move the buttons under
   *  a pointer already travelling towards one. */
  test('every month gets six rows, however short', () => {
    // February 2026 is 28 days beginning on a Sunday: exactly four weeks
    // under a Sunday start, the shortest a month can be.
    expect(monthGrid({ y: 2026, m: 2, d: 1 }, 'sunday')).toHaveLength(42);
    expect(monthGrid({ y: 2026, m: 8, d: 1 }, 'saturday')).toHaveLength(42);
  });
});
