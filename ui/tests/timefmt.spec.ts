import { test, expect } from '@playwright/test';
import { formatClock, gutterLabel } from '../src/lib/timefmt';

// A table of inputs to outputs, run in the Node process with no `page` — the
// same shape as `ink.spec.ts` and `position.spec.ts`, and for the same reason:
// this is arithmetic, and the *rendered* half (that the setting reaches the
// blocks and the ruler) belongs in `components.spec.ts` and `app.spec.ts`.

/** An instant at a given local hour and minute. Built from the browser's own
 *  zone rather than from a UTC epoch, because that is the zone `formatClock`
 *  reads — a fixture in UTC would be testing this machine's offset. */
const at = (hour: number, minute: number) => {
  const d = new Date(2026, 7, 14, hour, minute, 0, 0);
  return d.getTime();
};

test.describe('formatClock', () => {
  test('the 24-hour clock is what it has always been, zero-padded on both halves', () => {
    expect(formatClock(at(13, 30), '24h')).toBe('13:30');
    expect(formatClock(at(0, 5), '24h')).toBe('00:05');
    expect(formatClock(at(9, 0), '24h')).toBe('09:00');
    expect(formatClock(at(23, 59), '24h')).toBe('23:59');
  });

  // **The four rows that matter.** Midnight and noon are the cases every
  // hand-rolled 12-hour clock gets wrong, in both directions: `h % 12` alone
  // prints `0:05 AM` for midnight and `0:05 PM` for noon, and a naive
  // `h < 12 ? 'AM'` applied to the *dial* hour rather than the 24-hour one
  // flips noon to AM. Neither mistake is visible anywhere else on the clock —
  // 1am through 11pm agree under all of them.
  test('midnight and noon are twelve, and they are the only hours that can be', () => {
    expect(formatClock(at(0, 5), '12h')).toBe('12:05 AM');
    expect(formatClock(at(12, 5), '12h')).toBe('12:05 PM');
    expect(formatClock(at(11, 59), '12h')).toBe('11:59 AM');
    expect(formatClock(at(13, 0), '12h')).toBe('1:00 PM');
  });

  test('a 12-hour hour carries no leading zero, and its minute always does', () => {
    expect(formatClock(at(9, 5), '12h')).toBe('9:05 AM');
    expect(formatClock(at(1, 0), '12h')).toBe('1:00 AM');
  });

  // Every hour of the day, so a table cannot be right in the four rows above
  // and wrong in the twenty between them.
  test('every hour of the day is AM before noon and PM from noon', () => {
    for (let h = 0; h < 24; h++) {
      const out = formatClock(at(h, 0), '12h');
      expect(out.endsWith(h < 12 ? 'AM' : 'PM'), `hour ${h} → ${out}`).toBe(true);
      const dial = Number(out.split(':')[0]);
      expect(dial >= 1 && dial <= 12, `hour ${h} → ${out} is off the dial`).toBe(true);
    }
  });
});

test.describe('gutterLabel', () => {
  test('the 24-hour ruler is the zero-padded hour it has always been', () => {
    expect(gutterLabel(0, '24h')).toBe('00');
    expect(gutterLabel(9, '24h')).toBe('09');
    expect(gutterLabel(23, '24h')).toBe('23');
  });

  test('the 12-hour ruler is compact, and turns over at noon', () => {
    expect(gutterLabel(0, '12h')).toBe('12a');
    expect(gutterLabel(11, '12h')).toBe('11a');
    expect(gutterLabel(12, '12h')).toBe('12p');
    expect(gutterLabel(23, '12h')).toBe('11p');
  });

  // The gutter is a fixed-width column: a label that grew would push the grid.
  test('no ruler label exceeds three characters', () => {
    for (let h = 0; h < 24; h++) {
      expect(gutterLabel(h, '12h').length).toBeLessThanOrEqual(3);
      expect(gutterLabel(h, '24h').length).toBe(2);
    }
  });
});
