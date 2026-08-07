import { invoke } from '@tauri-apps/api/core';

export type Attendee = {
  email: string;
  display_name: string | null;
  response_status: string;
  optional: boolean;
  is_self: boolean;
};

export type EventDetail = {
  id: number;
  calendar_id: number;
  title: string | null;
  description: string | null;
  location: string | null;
  conference_uri: string | null;
  start_ms: number;
  end_ms: number;
  is_all_day: boolean;
  is_recurring: boolean;
  /** The raw `RRULE`, carried through unchanged so the UI can tell a rule it
   *  can represent from one it cannot. */
  recurrence: string | null;
  color: string | null;
  organizer_email: string | null;
  self_response: string | null;
  can_respond: boolean;
  can_edit: boolean;
  attendees: Attendee[];
};

export const getEventDetail = (id: number) => invoke<EventDetail>('event_detail', { id });

/**
 * What `create_event` takes on the Rust side (`write::EventInput`) — the
 * UI's own vocabulary, not an RRULE: `repeat` is one of `'never'`,
 * `'daily'`, `'weekdays'`, `'weekly'`, `'monthly'`, `'yearly'`, mapped to an
 * actual rule by `write::rrule_for`. Omit it entirely to create a one-off
 * event; `'never'` and "omitted" are the same thing on a create, since there
 * is no existing rule to leave alone.
 */
export type EventInput = {
  summary: string | null;
  location: string | null;
  description: string | null;
  startMs: number;
  endMs: number;
  isAllDay: boolean;
  tz: string;
  repeat?: string;
};

/** A freshness check on an already-open popover, not a load — see `WeekGrid`,
 *  which fires this after paint and ignores a rejection. */
export const refreshEvent = (id: number) => invoke<EventDetail>('refresh_event', { id });

/**
 * `occurrenceStartMs` is the `start_ms` of the block that was actually
 * clicked — the `UiEvent` from the grid, never `detail.start_ms`.
 *
 * For a recurring series, every expanded occurrence shares its master's
 * store row id (`commands::to_ui`), and `event_detail_impl` sets a
 * `EventDetail`'s `start_ms` to `event.start_utc`, which for that master row
 * *is* the series DTSTART. Passing `detail.start_ms` here type-checks and
 * reads correctly, and it silently patches occurrence #0 for everyone —
 * `sendUpdates=all`, so the wrong date's decline goes out to the whole guest
 * list. The caller must thread the clicked block's own `start_ms` through
 * instead, alongside the anchor rect it already carries for positioning.
 */
export const respondToEvent = (
  id: number,
  response: string,
  scope: 'this' | 'all',
  occurrenceStartMs: number,
) => invoke<EventDetail>('respond_to_event', { id, response, scope, occurrenceStartMs });

/** Creates a new event on `calendarId` and returns its freshly-written detail. */
export const createEvent = (calendarId: number, fields: EventInput) =>
  invoke<EventDetail>('create_event', { calendarId, fields });
