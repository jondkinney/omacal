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

/**
 * Saves an edit and returns the freshly-written detail.
 *
 * `occurrenceStartMs` carries the same rule as `respondToEvent`'s, and carries
 * it harder: it is the clicked block's own `start_ms`, never
 * `detail.start_ms`. For a recurring series that second value is the master
 * row's DTSTART, and an edit aimed at it patches occurrence #0 — with the
 * whole event as the payload and `sendUpdates=all` mailing the result to every
 * guest.
 *
 * `fields` is the form's whole state, not a diff: what the user actually
 * changed is worked out on the Rust side, against the event as it was loaded,
 * so a field nobody touched is never sent. `repeat` left out means "the user
 * did not touch Repeat" and the event's existing rule — which may be one this
 * app cannot express — is left alone.
 *
 * **The invariant the whole thing rests on:** when the user has not touched
 * the time, `fields.startMs` must equal `occurrenceStartMs` *exactly*. The
 * Rust side reads a time change as the difference between the two and applies
 * that movement to whatever resource the scope resolves to, so two equal
 * values mean "no movement" and no `start`/`end` is sent at all. Pass
 * `detail.start_ms` as the anchor — the mistake this file already warns about
 * above — and a recurring event's untouched time reads as a move of weeks:
 * with scope `'all'` that drags the series' first occurrence onto the edited
 * date and drops everything before it. Both values must come from the same
 * clicked block.
 *
 * `'following'` splits the series in two: a new one starting at the clicked
 * occurrence and carrying `fields`, and the original shortened to end just
 * before it. That is two writes with no transaction across them, so it is the
 * one scope that can partly succeed — it reports a leftover duplicate rather
 * than pretending otherwise, and the message is meant to be shown as-is. It is
 * also the one scope that notifies guests of a *creation*: the new series
 * carries the whole guest list of the one it continues.
 *
 * `'following'` also refuses two shapes outright, before writing anything, and
 * both messages are written to be shown to the user as they are: a series that
 * ends after a set number of times, and one whose later occurrences have been
 * moved or deleted on their own — a split cannot carry those across, and losing
 * them silently would be worse than not splitting.
 *
 * A scope this command does not implement is refused outright rather than
 * treated as "this occurrence".
 */
export const updateEvent = (
  id: number,
  scope: 'this' | 'all' | 'following',
  occurrenceStartMs: number,
  fields: EventInput,
) => invoke<EventDetail>('update_event', { id, scope, occurrenceStartMs, fields });

/**
 * Deletes an event, or part of a recurring series, and resolves with nothing.
 *
 * `occurrenceStartMs` carries the same rule as `respondToEvent`'s and
 * `updateEvent`'s, and carries it hardest of the three: it is the clicked
 * block's own `start_ms`, never `detail.start_ms`. For a recurring series that
 * second value is the master row's DTSTART, so a `'this'` delete aimed at it
 * removes the series' *first* occurrence rather than the one on screen.
 *
 * The three scopes are three different operations, not three sizes of the same
 * one:
 *
 * - `'this'` removes the clicked occurrence only. The rest of the series stays.
 * - `'all'` removes the whole series, past occurrences included.
 * - `'following'` shortens the series to end just before the clicked
 *   occurrence. Nothing is deleted: the occurrences before it are in the same
 *   Google event, so deleting it would take them too.
 *
 * **Every one of them notifies the guest list** — the delete goes out with
 * `sendUpdates=all`, because a meeting that vanishes for the organiser alone is
 * worse than an email. A confirmation shown before calling this should say so,
 * and can say how many people from `detail.attendees`.
 *
 * There is no undo. Google keeps no copy this app can reach, and for `'all'` the
 * past occurrences go with the series.
 *
 * Nothing is returned, so the caller reloads the grid itself. For `'this'` on a
 * series that reload has to be a *sync* rather than a re-read: the local store
 * cannot know an occurrence is gone until Google says so, so the block stays on
 * screen until the next sync picks up the cancellation.
 *
 * A scope this command does not implement is refused outright rather than
 * treated as "this occurrence".
 */
export const deleteEvent = (
  id: number,
  scope: 'this' | 'all' | 'following',
  occurrenceStartMs: number,
) => invoke<void>('delete_event_cmd', { id, scope, occurrenceStartMs });
