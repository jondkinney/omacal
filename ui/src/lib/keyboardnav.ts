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

export type NavigationClock = {
  nowMs: number;
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
 * Move through events in reading order. From a selected day other than today,
 * `j` takes its first event and `k` its last. Today is time-aware when a
 * clock is supplied: `j` starts with the first timed event that has not ended
 * and `k` with the last timed event that has begun. All-day rows have no useful
 * position relative to the current clock, so this initial-today check ignores
 * them. If there is no eligible timed event today, traversal continues into
 * the adjacent day instead of replaying elapsed events or selecting an all-day
 * row merely because its stored midnight-to-midnight span contains `now`.
 * Once an event is selected, movement is strictly adjacent—including all-day
 * rows—and the clock no longer participates.
 */
export function moveEvent(
  days: ListDay[],
  cursor: KeyboardCursor,
  dir: -1 | 1,
  clock: NavigationClock | null = null,
): CursorMove {
  const dayIndex = days.findIndex((d) => d.startMs === cursor.dayStartMs);
  if (dayIndex === -1) return { cursor, overflow: dir };

  const day = days[dayIndex];
  const dayOnly = cursor.eventId === null || cursor.eventStartMs === null;
  const eventIndex = dayOnly
    ? -1
    : day.events.findIndex((event) => cursorNamesEvent(cursor, day.startMs, event));

  if (dayOnly && day.events.length > 0) {
    let event: UiEvent | undefined;
    // The payload's own interval names today. Comparing `startMs` with a
    // browser-derived midnight was subtly wrong whenever the display calendar
    // and browser/system zones differed; adding 24 hours would also be wrong
    // on a DST boundary. The backend already supplied both true boundaries.
    if (clock && clock.nowMs >= day.startMs && clock.nowMs < day.endMs) {
      if (dir === 1) {
        event = day.events.find(
          (candidate) => !candidate.is_all_day && candidate.end_ms > clock.nowMs,
        );
      } else {
        for (let i = day.events.length - 1; i >= 0; i -= 1) {
          if (!day.events[i].is_all_day && day.events[i].start_ms <= clock.nowMs) {
            event = day.events[i];
            break;
          }
        }
      }
    } else {
      event = dir === 1 ? day.events[0] : day.events[day.events.length - 1];
    }
    if (event) return { cursor: eventCursor(day, event), overflow: null };
  } else if (eventIndex === -1 && day.events.length > 0) {
    // A stale event identity is treated as its day selection. It is not the
    // initial-today path above, so a clock tick must not skip visible rows.
    const event = dir === 1 ? day.events[0] : day.events[day.events.length - 1];
    return { cursor: eventCursor(day, event), overflow: null };
  }

  if (!dayOnly) {
    const nextInDay = day.events[eventIndex + dir];
    if (nextInDay) return { cursor: eventCursor(day, nextInDay), overflow: null };
  }

  for (let i = dayIndex + dir; i >= 0 && i < days.length; i += dir) {
    if (days[i].events.length === 0) continue;
    const event = dir === 1 ? days[i].events[0] : days[i].events[days[i].events.length - 1];
    return { cursor: eventCursor(days[i], event), overflow: null };
  }
  return { cursor, overflow: dir };
}
