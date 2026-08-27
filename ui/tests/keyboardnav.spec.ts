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

  test('j walks down all-day and timed rows, then enters the next day', () => {
    const list = days();
    const first = moveEvent(list, dayCursor(APP_MON), 1);
    expect(eventAtCursor(list, first.cursor)?.title).toBe('All-day planning');

    const second = moveEvent(list, first.cursor, 1);
    expect(eventAtCursor(list, second.cursor)?.title).toBe('Plan the launch');

    const third = moveEvent(list, second.cursor, 1);
    expect(eventAtCursor(list, third.cursor)?.title).toBe('Review notes');

    const nextDay = moveEvent(list, third.cursor, 1);
    expect(nextDay.cursor.dayStartMs).toBe(APP_MON + DAY);
    expect(eventAtCursor(list, nextDay.cursor)?.title).toBe('Tuesday brief');
  });

  test('j from today skips events that have already ended', () => {
    const list = days();
    const lateMonday = APP_MON + 22 * 3_600_000;
    const next = moveEvent(list, dayCursor(APP_MON), 1, { nowMs: lateMonday });

    expect(next.cursor.dayStartMs).toBe(APP_MON + DAY);
    expect(eventAtCursor(list, next.cursor)?.title).toBe('Tuesday brief');
  });

  test('j from late today ignores an all-day row and advances to tomorrow', () => {
    const list = days();
    expect(list[0].events.some((event) => event.is_all_day)).toBe(true);
    const lateMonday = APP_MON + 22 * 3_600_000;
    const next = moveEvent(list, dayCursor(APP_MON), 1, { nowMs: lateMonday });

    expect(next.cursor.dayStartMs).toBe(APP_MON + DAY);
    expect(eventAtCursor(list, next.cursor)?.title).toBe('Tuesday brief');
  });

  test('k at the top enters the last event of the previous day', () => {
    const list = days();
    const tuesdayLast = moveEvent(list, dayCursor(APP_MON + DAY), -1);
    expect(eventAtCursor(list, tuesdayLast.cursor)?.title).toBe('Tuesday brief');

    const mondayLast = moveEvent(list, tuesdayLast.cursor, -1);
    expect(mondayLast.cursor.dayStartMs).toBe(APP_MON);
    expect(eventAtCursor(list, mondayLast.cursor)?.title).toBe('Review notes');
  });

  test('k from today skips events that have not started yet', () => {
    const list = days();
    const earlyTuesday = APP_MON + DAY + 8 * 3_600_000;
    const previous = moveEvent(list, dayCursor(APP_MON + DAY), -1, { nowMs: earlyTuesday });

    expect(previous.cursor.dayStartMs).toBe(APP_MON);
    expect(eventAtCursor(list, previous.cursor)?.title).toBe('Review notes');
  });

  test('k from early today ignores an all-day row and retreats to yesterday', () => {
    const list = days();
    const source = list[3].events.find((event) => event.is_all_day)!;
    list[1] = {
      ...list[1],
      events: [
        {
          ...source,
          id: 906,
          title: 'Today all day',
          start_ms: APP_MON + DAY,
          end_ms: APP_MON + 2 * DAY,
        },
        ...list[1].events,
      ],
    };
    const earlyTuesday = APP_MON + DAY + 8 * 3_600_000;
    const previous = moveEvent(list, dayCursor(APP_MON + DAY), -1, { nowMs: earlyTuesday });

    expect(previous.cursor.dayStartMs).toBe(APP_MON);
    expect(eventAtCursor(list, previous.cursor)?.title).toBe('Review notes');
  });

  test('the payload day interval, not the browser midnight, decides which day is today', () => {
    const list = days();
    const lateMonday = APP_MON + 22 * 3_600_000;

    // A calendar can display a zone other than the browser/system zone. Its
    // midnight is then a different instant, but 10 PM is still inside the
    // day interval the payload supplied. This is the live-app failure the
    // previous exact-midnight comparison missed.
    const next = moveEvent(list, dayCursor(APP_MON), 1, { nowMs: lateMonday });

    expect(next.cursor.dayStartMs).toBe(APP_MON + DAY);
    expect(eventAtCursor(list, next.cursor)?.title).toBe('Tuesday brief');
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
