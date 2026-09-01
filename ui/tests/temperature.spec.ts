import { expect, test } from '@playwright/test';
import { formatTemp } from '../src/lib/temperature';

/** The one place a Celsius reading gets rounded — pinned in both units so a
 *  regression to "round then convert" (off by a degree on a fraction like
 *  31.6) fails here rather than only in a screenshot. */
test.describe('formatTemp', () => {
  test('celsius is a pass-through round', () => {
    expect(formatTemp(31.6, 'celsius')).toBe('32');
    expect(formatTemp(-3, 'celsius')).toBe('-3');
    expect(formatTemp(0, 'celsius')).toBe('0');
  });

  test('fahrenheit converts before rounding, not after', () => {
    // 31.6°C is 88.88°F, which rounds to 89 — not 90, the answer you get by
    // rounding 31.6 to 32°C first and converting that.
    expect(formatTemp(31.6, 'fahrenheit')).toBe('89');
    expect(formatTemp(0, 'fahrenheit')).toBe('32');
    expect(formatTemp(-3, 'fahrenheit')).toBe('27');
    expect(formatTemp(100, 'fahrenheit')).toBe('212');
  });
});
