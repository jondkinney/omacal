import { test, expect } from '@playwright/test';
import type { WeekPayload } from '../src/lib/api';
import { padFor, panCommit, sliceWeek, snapPlan, visibleIndex } from '../src/lib/weekwindow';

const DAY = 86_400_000;
const days = (n: number, from = 0) =>
  Array.from({ length: n }, (_, i) => ({ start_ms: (from + i) * DAY, end_ms: (from + i + 1) * DAY, events: [], placed: [] }));
const lane = (start_col: number, end_col: number, idx = 0) =>
  ({ idx, lane: 0, start_col, end_col, cont_left: false, cont_right: false });

test.describe('the window on a padded week', () => {
  test('padding is the window\'s own width, and never less than three days', () => {
    expect(padFor(7)).toBe(7);
    expect(padFor(5)).toBe(5);
    expect(padFor(3)).toBe(3);
    expect(padFor(1)).toBe(3);
  });

  test('the window is found by its first day, and a payload without it says so', () => {
    expect(visibleIndex(days(21), 7 * DAY)).toBe(7);
    expect(visibleIndex(days(21), 0)).toBe(0);
    // A payload from before the window jumped, or an unpadded stub: the
    // callers show it whole rather than a window's worth of its start.
    expect(visibleIndex(days(7, 3), 50 * DAY)).toBe(-1);
  });

  test('slicing keeps the days in the window and cuts the lanes to it', () => {
    const week: WeekPayload = {
      days: days(21),
      all_day: [
        lane(5, 9, 0),   // starts in the padding, runs into the window
        lane(8, 10, 1),  // inside
        lane(12, 16, 2), // runs out the far side
        lane(0, 3, 3),   // padding only
        lane(2, 20, 4),  // straddles the whole window
      ],
      all_day_events: [],
      overflow: [4],
    };
    const w = sliceWeek(week, 7, 7);
    expect(w.days.map((d) => d.start_ms / DAY)).toEqual([7, 8, 9, 10, 11, 12, 13]);
    expect(w.all_day).toEqual([
      { ...lane(0, 2, 0), cont_left: true },
      lane(1, 3, 1),
      { ...lane(5, 6, 2), cont_right: true },
      { ...lane(0, 6, 4), cont_left: true, cont_right: true },
    ]);
    // Left whole: lanes index into the events, and the overflow is the
    // wider packing's judgement.
    expect(w.all_day_events).toBe(week.all_day_events);
    expect(w.overflow).toBe(week.overflow);
  });

  test('a lane already marked continuing stays so after the cut', () => {
    const week: WeekPayload = {
      days: days(21), all_day: [{ ...lane(7, 9), cont_left: true }], all_day_events: [], overflow: [],
    };
    expect(sliceWeek(week, 7, 7).all_day).toEqual([{ ...lane(0, 2), cont_left: true }]);
  });

  test('the whole payload slices to itself', () => {
    const week: WeekPayload = { days: days(7), all_day: [lane(1, 2)], all_day_events: [], overflow: [] };
    expect(sliceWeek(week, 0, 7)).toBe(week);
  });

  test('whole columns crossed are handed up against the travel, the fraction stays', () => {
    // Content moved 1.3 columns right: the window moves a day earlier.
    expect(panCommit(1.3)).toEqual({ shift: -1, rest: expect.closeTo(0.3, 9) });
    // Content moved 2.5 columns left: two days later.
    expect(panCommit(-2.5)).toEqual({ shift: 2, rest: -0.5 });
    expect(panCommit(0.9)).toEqual({ shift: 0, rest: 0.9 });
  });

  test('the fingers lifting settle on the nearest column, and one more past half', () => {
    expect(snapPlan(0.3)).toEqual({ shift: 0, from: expect.closeTo(0.3, 9) });
    expect(snapPlan(0.6)).toEqual({ shift: -1, from: expect.closeTo(-0.4, 9) });
    expect(snapPlan(-0.6)).toEqual({ shift: 1, from: expect.closeTo(0.4, 9) });
    expect(snapPlan(0)).toEqual({ shift: 0, from: 0 });
  });
});
