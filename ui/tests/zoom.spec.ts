import { test, expect } from '@playwright/test';
import {
  HOUR_PX_DEFAULT, HOUR_PX_MAX, HOUR_PX_MIN,
  clampHourPx, hourPxAfterPinch, hourPxAfterWheel, hourPxStepped, scrollTopKeeping,
} from '../src/lib/zoom';

test.describe('hour zoom arithmetic', () => {
  test('the range holds at both ends, and the default is the grid\'s own 70', () => {
    expect(clampHourPx(HOUR_PX_MAX * 3)).toBe(HOUR_PX_MAX);
    expect(clampHourPx(1)).toBe(HOUR_PX_MIN);
    expect(clampHourPx(73.4)).toBe(73.4); // the fraction survives: see the comment on it
    expect(HOUR_PX_DEFAULT).toBe(70); // 24 x 70 = the 1680px column every golden was drawn at
  });

  test('a wheel notch up zooms in, down zooms out, by the same factor', () => {
    const taller = hourPxAfterWheel(70, -100);
    const shorter = hourPxAfterWheel(70, 100);
    expect(taller).toBeGreaterThan(70);
    expect(shorter).toBeLessThan(70);
    expect(taller / 70).toBeCloseTo(70 / shorter, 6);
    // A trackpad's 2px tick moves it a fraction — small, but not nothing,
    // which is what lets a slow two-finger scroll zoom at all.
    expect(hourPxAfterWheel(70, -2)).toBeGreaterThan(70);
    expect(hourPxAfterWheel(70, -2)).toBeLessThan(71);
  });

  test('a pinch scales the height it began at, never the one it is passing through', () => {
    expect(hourPxAfterPinch(70, 1.5)).toBe(105);
    expect(hourPxAfterPinch(70, 0.5)).toBe(35);
    expect(hourPxAfterPinch(70, 4)).toBe(HOUR_PX_MAX);
  });

  test('the keys step by a quarter and invert exactly', () => {
    expect(hourPxStepped(80, 1)).toBe(100);
    expect(hourPxStepped(100, -1)).toBe(80);
    expect(hourPxStepped(HOUR_PX_MAX, 1)).toBe(HOUR_PX_MAX);
    expect(hourPxStepped(HOUR_PX_MIN, -1)).toBe(HOUR_PX_MIN);
  });

  test('the instant under the anchor stays under it', () => {
    // 09:00 at 70px/h sits 630px into the content; scrolled to 400, it is
    // 230px below the pane's top. Doubling the hours puts it at 1260, so
    // keeping it 230px down means scrolling to 1030.
    expect(scrollTopKeeping(400, 230, 70, 140)).toBe(1030);
    // Nothing above the top to hold in place: never negative.
    expect(scrollTopKeeping(0, 100, 140, 70)).toBe(0);
  });
});
