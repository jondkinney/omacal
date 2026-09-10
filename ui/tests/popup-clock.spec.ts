import { test, expect } from '@playwright/test';
import { currentClock } from '../../packaging/omarchy-plugin/Timeline.mjs';
test('popup clock advances through midnight in the configured zone and time format', () => {
  const ms = Date.UTC(2026, 8, 8, 4, 59);
  expect(currentClock(ms, '12h', -18000)).toBe('11:59pm');
  expect(currentClock(ms + 60000, '12h', -18000)).toBe('12:00am');
  expect(currentClock(ms + 60000, '24h', -18000)).toBe('00:00');
});
