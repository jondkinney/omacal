import { test, expect } from '@playwright/test';
import { allDaysFromWeek } from '../src/lib/filmstrip';
import { dayCursor, eventAtCursor, moveDay, moveEvent } from '../src/lib/keyboardnav';
import { APP_MON, keyboardWeek } from './fixtures';

const DAY = 24 * 3_600_000;
const days = () => allDaysFromWeek(keyboardWeek(APP_MON));

test.describe('calendar keyboard cursor', () => {
  test('w and b move one visible day and clear the event selection', () => {
    const forward = moveDay(days(), dayCursor(APP_MON), 1);
    expect(forward).toEqual({ cursor: dayCursor(APP_MON + DAY), overflow: null });

    const back = moveDay(days(), forward.cursor, -1);
    expect(back).toEqual({ cursor: dayCursor(APP_MON), overflow: null });
  });

  test('j walks down a day, then enters the first event of the next day', () => {
    const list = days();
    const first = moveEvent(list, dayCursor(APP_MON), 1);
    expect(eventAtCursor(list, first.cursor)?.title).toBe('Plan the launch');

    const second = moveEvent(list, first.cursor, 1);
    expect(eventAtCursor(list, second.cursor)?.title).toBe('Review notes');

    const nextDay = moveEvent(list, second.cursor, 1);
    expect(nextDay.cursor.dayStartMs).toBe(APP_MON + DAY);
    expect(eventAtCursor(list, nextDay.cursor)?.title).toBe('Tuesday brief');
  });

  test('k at the top enters the last event of the previous day', () => {
    const list = days();
    const tuesdayLast = moveEvent(list, dayCursor(APP_MON + DAY), -1);
    expect(eventAtCursor(list, tuesdayLast.cursor)?.title).toBe('Tuesday brief');

    const mondayLast = moveEvent(list, tuesdayLast.cursor, -1);
    expect(mondayLast.cursor.dayStartMs).toBe(APP_MON);
    expect(eventAtCursor(list, mondayLast.cursor)?.title).toBe('Review notes');
  });

  test('cross-day event movement skips an empty day and includes all-day events', () => {
    const list = days();
    const fromTuesday = moveEvent(list, moveEvent(list, dayCursor(APP_MON + DAY), 1).cursor, 1);
    expect(fromTuesday.cursor.dayStartMs).toBe(APP_MON + 3 * DAY);
    expect(eventAtCursor(list, fromTuesday.cursor)?.title).toBe('Off-site');
  });

  test('reports a payload edge instead of wrapping around', () => {
    expect(moveDay(days(), dayCursor(APP_MON), -1).overflow).toBe(-1);
    const last = dayCursor(APP_MON + 6 * DAY);
    expect(moveEvent(days(), last, 1).overflow).toBe(1);
  });
});
