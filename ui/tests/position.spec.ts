import { test, expect } from '@playwright/test';
import { placePopover } from '../src/lib/position';

const VIEW = { width: 1200, height: 800 };
const POP = { width: 320, height: 400 };

test.describe('placePopover', () => {
  test('opens to the right of the anchor when there is room', () => {
    const p = placePopover({ top: 100, left: 200, width: 120, height: 40 }, POP, VIEW);
    expect(p.left).toBe(328); // 200 + 120 + 8
    expect(p.top).toBe(100);
  });

  test('flips to the left when it would run off the right edge', () => {
    const p = placePopover({ top: 100, left: 1000, width: 120, height: 40 }, POP, VIEW);
    expect(p.left).toBe(672); // 1000 - 320 - 8
  });

  test('clamps rather than flipping off the left edge too', () => {
    // A narrow viewport where neither side fits: stay on screen and overlap
    // rather than render half off it.
    const p = placePopover({ top: 100, left: 10, width: 40, height: 40 }, POP,
                           { width: 360, height: 800 });
    expect(p.left).toBeGreaterThanOrEqual(8);
    expect(p.left + POP.width).toBeLessThanOrEqual(360 - 8);
  });

  test('a low anchor lifts the popover to stay on screen', () => {
    const p = placePopover({ top: 700, left: 200, width: 120, height: 40 }, POP, VIEW);
    expect(p.top + POP.height).toBeLessThanOrEqual(800 - 8);
    expect(p.top).toBeGreaterThanOrEqual(8);
  });

  test('a popover taller than the viewport pins to the top', () => {
    const p = placePopover({ top: 300, left: 200, width: 120, height: 40 },
                           { width: 320, height: 900 }, VIEW);
    expect(p.top).toBe(8);
  });

  test('a popover wider than the viewport pins to the left', () => {
    const p = placePopover({ top: 100, left: 200, width: 120, height: 40 },
                           { width: 1300, height: 400 }, VIEW);
    expect(p.left).toBe(8);
  });
});
