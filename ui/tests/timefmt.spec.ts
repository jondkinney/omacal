import { expect, test } from '@playwright/test';
import { displayClock, parseClock } from '../src/lib/timefmt';

/** The pair behind the form's time fields: storage is always `HH:MM`, and
 *  only the *rendering* follows the clock setting — which is the whole fix
 *  for the native time input that rendered the system locale's clock on a
 *  grid drawn in the app's. */
test.describe('displayClock', () => {
  test('24h is the identity, 12h is a dial', () => {
    expect(displayClock('13:30', '24h')).toBe('13:30');
    expect(displayClock('13:30', '12h')).toBe('1:30 PM');
    expect(displayClock('00:05', '12h')).toBe('12:05 AM');
    expect(displayClock('12:00', '12h')).toBe('12:00 PM');
    // Not-storage passes through untouched — the field never invents.
    expect(displayClock('junk', '12h')).toBe('junk');
  });
});

test.describe('parseClock', () => {
  test('both clocks parse, and a suffix decides which one was spoken', () => {
    expect(parseClock('13:30')).toBe('13:30');
    expect(parseClock('9:30')).toBe('09:30');
    expect(parseClock('9')).toBe('09:00');
    expect(parseClock('9:30 pm')).toBe('21:30');
    expect(parseClock('9.30pm')).toBe('21:30');
    expect(parseClock(' 12 AM ')).toBe('00:00');
    expect(parseClock('12pm')).toBe('12:00');
    expect(parseClock('12:30 a.m.')).toBe('00:30');
  });

  test('what is not a time is null, never a guess', () => {
    expect(parseClock('24:00')).toBeNull();     // no 24 on either clock
    expect(parseClock('13:30 pm')).toBeNull();  // a dial has no 13
    expect(parseClock('9:5')).toBeNull();       // a typo, not five past nine
    expect(parseClock('9:75')).toBeNull();
    expect(parseClock('half past nine')).toBeNull();
    expect(parseClock('')).toBeNull();
  });
});
