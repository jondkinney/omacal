import { test, expect } from '@playwright/test';
import { formatDate, type DateFormat } from '../src/lib/datefmt';
test('date preferences preserve date-only values and respect display-zone boundaries', () => {
  const ms = Date.UTC(2026, 8, 7);
  for (const [format, expected] of Object.entries({ mdy: '09/07/2026', dmy: '07/09/2026', iso: '2026-09-07', 'long-mdy': 'Sep 7, 2026', 'long-dmy': '7 Sep 2026' })) {
    expect(formatDate(ms, format as DateFormat, { timeZone: 'UTC' })).toBe(expected);
  }
  expect(formatDate(ms, 'iso', { timeZone: 'America/Chicago' })).toBe('2026-09-06');
  expect(formatDate(Date.UTC(2028, 1, 29), 'dmy', { timeZone: 'UTC' })).toBe('29/02/2028');
});

