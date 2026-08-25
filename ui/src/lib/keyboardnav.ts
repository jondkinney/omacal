import type { UiEvent } from './api';
import type { ListDay } from './filmstrip';

/** The occurrence currently named by the calendar keyboard. `eventId` and
 * `eventStartMs` are null together when the day itself, rather than one of its
 * events, is selected. The day stays separate because a multi-day all-day
 * occurrence can appear in more than one column. */
export type KeyboardCursor = {
  dayStartMs: number;
  eventId: number | null;
  eventStartMs: number | null;
};

export type CursorMove = {
  cursor: KeyboardCursor;
  /** The payload ended before the requested destination. App uses this to
   * fetch the adjacent period; pure navigation never guesses its size. */
  overflow: -1 | 1 | null;
};

export function dayCursor(dayStartMs: number): KeyboardCursor {
  return { dayStartMs, eventId: null, eventStartMs: null };
}

export function cursorNamesEvent(cursor: KeyboardCursor, dayStartMs: number, event: UiEvent): boolean {
  return cursor.dayStartMs === dayStartMs
    && cursor.eventId === event.id
    && cursor.eventStartMs === event.start_ms;
}

export function eventAtCursor(days: ListDay[], cursor: KeyboardCursor): UiEvent | null {
  const day = days.find((d) => d.startMs === cursor.dayStartMs);
  if (!day || cursor.eventId === null || cursor.eventStartMs === null) return null;
  return day.events.find((event) => cursorNamesEvent(cursor, day.startMs, event)) ?? null;
}

function eventCursor(day: ListDay, event: UiEvent): KeyboardCursor {
  return { dayStartMs: day.startMs, eventId: event.id, eventStartMs: event.start_ms };
}

/** Move to the adjacent visible day and leave its event list unselected. */
export function moveDay(days: ListDay[], cursor: KeyboardCursor, dir: -1 | 1): CursorMove {
  const i = days.findIndex((d) => d.startMs === cursor.dayStartMs);
  if (i === -1) return { cursor, overflow: dir };
  const next = days[i + dir];
  return next
    ? { cursor: dayCursor(next.startMs), overflow: null }
    : { cursor, overflow: dir };
}

/**
 * Move through events in reading order. From a selected day, `j` takes its
 * first event and `k` its last. Crossing an edge skips empty days and lands on
 * the first/last event in the next non-empty day.
 */
export function moveEvent(days: ListDay[], cursor: KeyboardCursor, dir: -1 | 1): CursorMove {
  const dayIndex = days.findIndex((d) => d.startMs === cursor.dayStartMs);
  if (dayIndex === -1) return { cursor, overflow: dir };

  const day = days[dayIndex];
  const eventIndex = cursor.eventId === null || cursor.eventStartMs === null
    ? -1
    : day.events.findIndex((event) => cursorNamesEvent(cursor, day.startMs, event));

  if (eventIndex === -1 && day.events.length > 0) {
    const event = dir === 1 ? day.events[0] : day.events[day.events.length - 1];
    return { cursor: eventCursor(day, event), overflow: null };
  }

  const nextInDay = day.events[eventIndex + dir];
  if (nextInDay) return { cursor: eventCursor(day, nextInDay), overflow: null };

  for (let i = dayIndex + dir; i >= 0 && i < days.length; i += dir) {
    if (days[i].events.length === 0) continue;
    const event = dir === 1 ? days[i].events[0] : days[i].events[days[i].events.length - 1];
    return { cursor: eventCursor(days[i], event), overflow: null };
  }
  return { cursor, overflow: dir };
}
