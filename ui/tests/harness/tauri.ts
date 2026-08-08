// A stand-in for the Tauri IPC layer, so `App.svelte` — four `$effect`s, two
// event listeners, and every interaction the final review flagged — can be
// mounted and driven from a spec.
//
// Both halves of the app's Tauri surface bottom out in one object:
// `invoke()` is `window.__TAURI_INTERNALS__.invoke(...)`, and `listen()` is
// itself an `invoke('plugin:event|listen', …)` carrying a callback registered
// through `window.__TAURI_INTERNALS__.transformCallback`. Replacing that one
// object therefore stubs commands and events together, with no module mocking
// and no build-time aliasing — the app imports the real `@tauri-apps/api`.

import type { WeekPayload, MonthPayload, YearPayload, BigYearPayload } from '../../src/lib/api';
import type { AppStatus } from '../../src/lib/status';
import type { Calendar } from '../../src/lib/calendars';
import type { EventDetail } from '../../src/lib/eventdetail';
import {
  labelledWeek, weekLabel, APP_FIVE_MIN_AGO, POPOVER_DETAILS, busyDayMonth,
  appWritableWeek, APP_WRITE_CALENDARS, CREATED_DETAIL, crossZoneWeek,
  XZONE_WEEK_START,
} from '../fixtures';

/** What the real `get_palette` returns; the same fallback_dark values. */
const PALETTE = {
  bg: '#17171a', surface: '#1e1e22', text: '#e8e8ea',
  muted: '#8a8a90', accent: '#5b8def', is_dark: true,
};

/** The first-run failure a user is most likely to meet: no config file. */
export const NO_CONFIG_ERROR =
  'no config at /Users/someone/.config/omacal/config.toml: No such file or directory ' +
  '(os error 2). Create it with client_id and client_secret.';

type Deferred = { resolve: (w: WeekPayload) => void; reject: (e: unknown) => void };

export type Harness = {
  /**
   * Fire a Tauri event at the app, once it is actually listening.
   *
   * `listen()` is itself an async round trip, so an event fired the instant
   * after `goto` can land before the app has subscribed and simply vanish —
   * which showed up as a spec that passed under WebKit and failed under
   * Chromium. Waiting for the subscription makes the ordering a fact rather
   * than a hope; a missing subscriber is a real failure and throws.
   */
  emit(event: string, payload: unknown): Promise<void>;
  /** Park the *next* `get_week` for this week start instead of answering it. */
  hold(weekStartMs: number): void;
  /** How many `get_week` calls are currently parked. */
  held(): number;
  /** Answer a parked `get_week`, then let its `.then` chain run. */
  release(weekStartMs: number): Promise<void>;
  /** Make the next `get_week` reject, whoever asks for it. */
  failNextWeek(message: string): void;
  /** Make the next call to `set_calendar_selected` or `set_calendar_sync` reject —
   *  what a CalendarPopover spec uses to drive the failed-toggle path. */
  failNextCalendarCall(cmd: 'set_calendar_selected' | 'set_calendar_sync', message: string): void;
  /** Park the *next* call to this calendar command instead of answering it —
   *  what a CalendarPopover spec uses to prove a second row's toggle doesn't
   *  free up a first row whose own call is still pending. */
  holdNextCalendarCall(cmd: 'set_calendar_selected' | 'set_calendar_sync'): void;
  /** Answer the parked call for this command, then let its `.then` chain run. */
  releaseCalendarCall(cmd: 'set_calendar_selected' | 'set_calendar_sync', value: unknown): Promise<void>;
  /** Park the *next* `event_detail`, `refresh_event`, or `respond_to_event`
   *  call for this exact event id instead of answering it — what a
   *  `WeekGrid` spec uses to drive the supersession guard (open one block
   *  while another's detail is still loading), the after-paint refresh
   *  (control when it lands), and closing a popover while its own RSVP is
   *  still in flight (control when *that* lands). */
  holdNextEventCall(cmd: 'event_detail' | 'refresh_event' | 'respond_to_event', id: number): void;
  /** Answer the parked call for this command and id, then let its `.then`
   *  chain — including `WeekGrid`'s own `detail = fresh` — actually run. */
  releaseEventCall(
    cmd: 'event_detail' | 'refresh_event' | 'respond_to_event',
    id: number,
    value: EventDetail,
  ): Promise<void>;
  /** Make the next call to this command for this exact id reject — what a
   *  `WeekGrid` spec uses to drive the load-failure-closes path
   *  (`event_detail`) without disturbing every other id's fixture. */
  failNextEventCall(cmd: 'event_detail' | 'refresh_event' | 'respond_to_event', id: number, message: string): void;
  /** Reject the *parked* call for this command and id — the counterpart to
   *  `releaseEventCall`, for proving a *late failure* for a superseded
   *  click does not close a popover that opened successfully after it.
   *  `failNextEventCall` alone cannot drive that: it rejects immediately,
   *  before a second click could ever land. */
  rejectEventCall(cmd: 'event_detail' | 'refresh_event' | 'respond_to_event', id: number, message: string): Promise<void>;
  /** Every command the app has invoked, in order. */
  calls: { cmd: string; args: unknown }[];
};

/** What `set_calendar_sync(id, false)` reports removing, absent a forced failure. */
export const CALENDAR_SYNC_REMOVED = 143;

/** A well-shaped but otherwise unused `EventDetail`, returned by the
 *  `respond_to_event` stub — see the case below for why its content never
 *  matters to a spec. */
const RESPOND_STUB_DETAIL = {
  id: 0, title: null, description: null, location: null, conference_uri: null,
  start_ms: 0, end_ms: 0, is_all_day: false, is_recurring: false, color: null,
  recurrence: null, repeat: 'never',
  organizer_email: null, self_response: null, can_respond: true, can_edit: false,
  attendees: [],
};

const listeners = new Map<string, Set<(e: unknown) => void>>();
const callbacks = new Map<number, (e: unknown) => void>();
const hold = new Set<number>();
const parked = new Map<number, Deferred>();

type CalendarDeferred = { resolve: (v: unknown) => void; reject: (e: unknown) => void };

let nextId = 1;
let failWeekOnce: string | null = null;
let failCalendarOnce: { cmd: string; message: string } | null = null;
let holdCalendarOnce: string | null = null;
const parkedCalendar = new Map<string, CalendarDeferred>();

type EventDeferred = { resolve: (d: EventDetail) => void; reject: (e: unknown) => void };
type EventCmd = 'event_detail' | 'refresh_event' | 'respond_to_event';

// Keyed by `${cmd}:${id}`, not just `id`: a spec driving the after-paint
// refresh needs to hold `refresh_event` for an id while `event_detail` for
// that same id answers normally, and a single `id`-only key couldn't tell
// the two commands' holds apart.
function eventCallKey(cmd: EventCmd, id: number): string {
  return `${cmd}:${id}`;
}
const holdEventCallOnce = new Set<string>();
const parkedEventCall = new Map<string, EventDeferred>();
let failEventCallOnce: { key: string; message: string } | null = null;

/**
 * Resolves once something has subscribed to `event`; throws if none does.
 * Counts polls rather than watching the clock — the specs freeze `Date.now()`.
 */
async function whenListening(event: string, polls = 300): Promise<void> {
  for (let i = 0; i < polls; i++) {
    if (listeners.get(event)?.size) return;
    await new Promise((r) => setTimeout(r, 10));
  }
  throw new Error(`nothing is listening for "${event}"`);
}

const harness: Harness = {
  calls: [],
  async emit(event, payload) {
    await whenListening(event);
    for (const fn of listeners.get(event) ?? []) fn({ event, id: 0, payload });
  },
  hold(weekStartMs) {
    hold.add(weekStartMs);
  },
  held() {
    return parked.size;
  },
  async release(weekStartMs) {
    parked.get(weekStartMs)?.resolve(labelledWeek(weekStartMs));
    parked.delete(weekStartMs);
    // Let the resolution — and anything it schedules — actually run, so a
    // spec asserting "the stale response did not land" is asserting about a
    // response that has had its chance rather than one still in the queue.
    await new Promise((r) => setTimeout(r, 50));
  },
  failNextWeek(message) {
    failWeekOnce = message;
  },
  failNextCalendarCall(cmd, message) {
    failCalendarOnce = { cmd, message };
  },
  holdNextCalendarCall(cmd) {
    holdCalendarOnce = cmd;
  },
  async releaseCalendarCall(cmd, value) {
    parkedCalendar.get(cmd)?.resolve(value);
    parkedCalendar.delete(cmd);
    // Same reasoning as `release` above: let the resolution's `.then` chain —
    // `onchange()`, `markBusy(id, false)`, the focus restore — actually run
    // before the spec asserts on it.
    await new Promise((r) => setTimeout(r, 50));
  },
  holdNextEventCall(cmd, id) {
    holdEventCallOnce.add(eventCallKey(cmd, id));
  },
  async releaseEventCall(cmd, id, value) {
    const key = eventCallKey(cmd, id);
    parkedEventCall.get(key)?.resolve(value);
    parkedEventCall.delete(key);
    await new Promise((r) => setTimeout(r, 50));
  },
  failNextEventCall(cmd, id, message) {
    failEventCallOnce = { key: eventCallKey(cmd, id), message };
  },
  async rejectEventCall(cmd, id, message) {
    const key = eventCallKey(cmd, id);
    parkedEventCall.get(key)?.reject(message);
    parkedEventCall.delete(key);
    await new Promise((r) => setTimeout(r, 50));
  },
};

/** Checks whether a spec armed a hold or a forced failure for `(cmd, id)`
 *  and, if so, returns the promise to answer with. `null` means neither is
 *  armed — the caller falls through to its own default response. Shared by
 *  `event_detail`/`refresh_event` (via `eventCallResult` below) and
 *  `respond_to_event` (inline in the command switch), which otherwise have
 *  nothing else in common: their "not held" defaults come from entirely
 *  different places. */
function heldOrFailed(cmd: EventCmd, id: number): Promise<EventDetail> | null {
  const key = eventCallKey(cmd, id);
  if (failEventCallOnce?.key === key) {
    const { message } = failEventCallOnce;
    failEventCallOnce = null;
    return Promise.reject(message);
  }
  if (holdEventCallOnce.has(key)) {
    holdEventCallOnce.delete(key);
    return new Promise<EventDetail>((resolve, reject) => {
      parkedEventCall.set(key, { resolve, reject });
    });
  }
  return null;
}

/** Resolves against `POPOVER_DETAILS` unless a spec armed a failure or a
 *  hold for this exact command and id. Both `event_detail` and
 *  `refresh_event` default to the same, unchanged entry — the ordinary
 *  case, where nothing moved since the popover opened. A *different* value
 *  (e.g. `POPOVER_REFRESHED_DETAIL`) only ever reaches a spec through an
 *  explicit `releaseEventCall(cmd, id, thatValue)`, never through this
 *  default path. */
function eventCallResult(cmd: 'event_detail' | 'refresh_event', id: number): Promise<EventDetail> {
  const held = heldOrFailed(cmd, id);
  if (held) return held;
  const d = POPOVER_DETAILS[id];
  if (!d) throw new Error(`no ${cmd} fixture for event id ${id}`);
  return Promise.resolve(d);
}

/** Resolves normally unless a spec armed a failure or a hold for this exact command. */
function calendarResult<T>(cmd: string, ok: T): Promise<T> {
  if (failCalendarOnce?.cmd === cmd) {
    const { message } = failCalendarOnce;
    failCalendarOnce = null;
    return Promise.reject(message);
  }
  if (holdCalendarOnce === cmd) {
    holdCalendarOnce = null;
    return new Promise<T>((resolve, reject) => {
      parkedCalendar.set(cmd, { resolve, reject } as CalendarDeferred);
    });
  }
  return Promise.resolve(ok);
}

/**
 * Command responses for a named scenario. `default` is a connected account on
 * a working backend; the rest exist to reach a failure the UI has to render.
 */
function statusFor(scenario: string): AppStatus {
  switch (scenario) {
    case 'no-config':
    case 'disconnected':
    // Starts with nobody connected, so `handleSignIn` runs the
    // "Connect Google Calendar" path rather than "Add account" — Task 7's
    // picker opens after either.
    case 'sign-in-adds-account':
      return { accounts: [], last_sync_ms: null, demo: false };
    default:
      return { accounts: ['me@x.com'], last_sync_ms: APP_FIVE_MIN_AGO, demo: false };
  }
}

/** What `get_calendars` returns for the `sign-in-adds-account` scenario, once
 *  `sign_in` has actually been called — one freshly imported account, all of
 *  it switched on by default, exactly as a real `sign_in` leaves it. */
const SIGNED_IN_CALENDARS: Calendar[] = [
  { id: 1, account_id: 1, account_email: 'new@x.com', summary: 'Personal',
    color_hex: '#5b8def', selected: true, sync_enabled: true, is_primary: true,
    access_role: 'owner' },
  // A subscribed holiday calendar really is a `reader`, and this is the one
  // fixture in the suite that stands in for a real `sign_in` import — so it
  // carries the role a real one would rather than a uniformly writable list.
  { id: 2, account_id: 1, account_email: 'new@x.com', summary: 'Holidays',
    color_hex: '#e2a03f', selected: true, sync_enabled: true, is_primary: false,
    access_role: 'reader' },
];

function getWeek(scenario: string, weekStartMs: number): Promise<WeekPayload> {
  if (failWeekOnce !== null) {
    const message = failWeekOnce;
    failWeekOnce = null;
    return Promise.reject(message);
  }
  if (hold.has(weekStartMs)) {
    hold.delete(weekStartMs);
    return new Promise<WeekPayload>((resolve, reject) => {
      parked.set(weekStartMs, { resolve, reject });
    });
  }
  // The `writable` scenario answers with the same two editable events whatever
  // week is asked for — same shortcut, and the same reasoning, as `getMonth`
  // below: its specs pin literal instants, and none of them needs the payload
  // to match the week it requested.
  if (scenario === 'writable') return Promise.resolve(appWritableWeek());
  // `cross-zone` is the one scenario where the week actually asked for is part
  // of the claim: its payload's columns are `Europe/Sofia` midnights, and it
  // only describes what is on screen if the app requested that same Monday.
  // Answering unconditionally would let a browser in the wrong zone — or an
  // `App` that opened on a different week — read this fixture's columns against
  // a header that had moved, with the agreement spec none the wiser. So it
  // refuses rather than substitutes, and the spec asserts the request too.
  if (scenario === 'cross-zone') {
    if (weekStartMs !== XZONE_WEEK_START) {
      return Promise.reject(
        `cross-zone: asked for week ${weekStartMs}, fixture is ${XZONE_WEEK_START}`,
      );
    }
    return Promise.resolve(crossZoneWeek());
  }
  return Promise.resolve(labelledWeek(weekStartMs));
}

const DAY_MS = 24 * 3_600_000;

/** Day view's own `get_day` stub: a one-column `WeekPayload`, echoing back
 *  whatever `dayStartMs` was actually asked for — same reasoning as
 *  `getWeek`'s `labelledWeek` above, just with no events to label. No App
 *  spec needs a populated day, only that the column it renders carries the
 *  date that was actually requested. */
function getDay(dayStartMs: number): WeekPayload {
  return {
    days: [{ start_ms: dayStartMs, end_ms: dayStartMs + DAY_MS, events: [], placed: [] }],
    all_day: [],
    all_day_events: [],
    overflow: [],
  };
}

/** Month view's own `get_month` stub. Unlike `getWeek`/`getDay`, this
 *  deliberately ignores `year`/`month` and always returns the same fixed
 *  grid (`MonthGrid`'s own `busy-day` fixture) — the anchor-survival spec
 *  pins a literal cell (`1_786_341_600_000`, Mon 10 Aug) as the value a
 *  click has to carry through to Day view, and no App spec needs the grid to
 *  actually match the requested month (that's `assemble_month`'s own
 *  Rust-side coverage, and `MonthGrid`'s). */
function getMonth(): MonthPayload {
  return busyDayMonth();
}

/** Year view's own `get_year` stub: twelve otherwise-empty months, echoing
 *  back whatever year was actually asked for — same reasoning as `getDay`'s
 *  `dayStartMs` above. No App spec needs a populated grid, only that it
 *  carries the year that was actually requested (`YearGrid`'s own `y2026`
 *  fixture already covers the populated case). */
function getYearStub(y: number): YearPayload {
  return {
    year: y,
    months: Array.from({ length: 12 }, (_, i) => ({ month: i + 1, lead_blanks: 0, days: [] })),
  };
}

/**
 * Big Year view's own `get_big_year` stub: fourteen otherwise-empty 28-day
 * rows, echoing back whatever year was actually asked for — same reasoning as
 * `getYearStub` above (`BigYearRibbon`'s own `y2026`/`crossing` fixtures
 * already cover the populated case).
 *
 * The days carry **real** `start_ms` values. They used to be `0` throughout,
 * on the reasoning that no App spec read them; Task 10 made a ribbon day
 * clickable, and a click through a day whose start is `0` opens an event form
 * dated 1 January 1970 — a fixture that would have made the create-from-Big-Year
 * witness assert the epoch and call it a pass. Anchored the way
 * `assemble_big_year` anchors: the Monday on or before 1 January of the year
 * asked for, then 28 days per row.
 */
function getBigYearStub(y: number): BigYearPayload {
  const jan1 = new Date(y, 0, 1);
  // `getDay()` is 0 for Sunday, so Sunday steps back six days, not none.
  const back = (jan1.getDay() + 6) % 7;
  const ribbonStart = new Date(y, 0, 1 - back).getTime();
  return {
    year: y,
    rows: Array.from({ length: 14 }, (_, r) => ({
      days: Array.from({ length: 28 }, (_, c) => ({
        start_ms: ribbonStart + (r * 28 + c) * DAY_MS,
        in_year: true,
        unsynced: false,
      })),
      pills: [],
      pill_events: [],
      overflow: [],
    })),
    legend: [],
  };
}

/** Installs the stub. Call before mounting anything that talks to Tauri. */
export function installTauriStub(scenario: string): Harness {
  // Reassigned by `sign_in` for the `sign-in-adds-account` scenario: a real
  // `sign_in` leaves the account durably connected, so the next `get_status`
  // must reflect it too, not just `get_calendars`.
  let status = statusFor(scenario);
  let signedIn = false;

  const invoke = async (cmd: string, args: Record<string, any> = {}): Promise<unknown> => {
    harness.calls.push({ cmd, args });
    switch (cmd) {
      case 'plugin:event|listen': {
        const fn = callbacks.get(args.handler);
        if (!fn) throw new Error(`listen with an unknown handler ${args.handler}`);
        const set = listeners.get(args.event) ?? new Set();
        set.add(fn);
        listeners.set(args.event, set);
        return args.handler; // the real one returns an event id; the id works
      }
      case 'plugin:event|unlisten': {
        const fn = callbacks.get(args.eventId);
        if (fn) listeners.get(args.event)?.delete(fn);
        return null;
      }
      case 'get_palette':
        return PALETTE;
      case 'get_status':
        return status;
      case 'get_week':
        return getWeek(scenario, args.weekStartMs);
      case 'get_day':
        return getDay(args.dayStartMs);
      case 'get_month':
        return getMonth();
      case 'get_year':
        return getYearStub(args.year);
      case 'get_big_year':
        return getBigYearStub(args.year);
      // App's own effect fetches calendars alongside status on mount. None of
      // the App specs exercise the popover, and Header only renders it once
      // `calendars.length > 0`, so an empty list keeps every existing
      // assertion undisturbed. CalendarPopover specs never take this path —
      // they mount the component directly with fixture props instead.
      case 'get_calendars':
        // `cross-zone` needs a writable list for the same reason `writable`
        // does: without one the form's calendar select has nothing to seed
        // from and Save refuses, so the edit half of its agreement spec could
        // never run.
        if (scenario === 'writable' || scenario === 'cross-zone') return APP_WRITE_CALENDARS;
        return scenario === 'sign-in-adds-account' && signedIn
          ? SIGNED_IN_CALENDARS
          : ([] as Calendar[]);
      case 'set_calendar_selected':
        return calendarResult(cmd, undefined);
      case 'set_calendar_sync':
        return calendarResult(cmd, CALENDAR_SYNC_REMOVED);
      case 'sync_now':
        return 0;
      case 'event_detail':
        return eventCallResult('event_detail', args.id);
      case 'refresh_event':
        return eventCallResult('refresh_event', args.id);
      case 'respond_to_event': {
        // Exposed so a spec can assert on exactly what `EventPopover` sent —
        // in particular, that the fourth argument is the clicked block's own
        // `start_ms` and never `detail.start_ms` (the task brief's trap).
        (window as any).__lastRespondCall = args;
        // Held/rejected the same way as event_detail/refresh_event — what a
        // WeekGrid spec uses to prove closing the popover mid-RSVP still
        // restyles the block once the response actually lands.
        const held = heldOrFailed('respond_to_event', args.id);
        if (held) return held;
        if (scenario === 'respond-fails') return Promise.reject('could not reach Google right now.');
        // Only the `writes-back` scenario simulates a real write-back (what
        // the backend actually returns for every non-recurring event, and
        // for `scope: 'all'`) — an attendee list carrying the new response,
        // so a spec can assert the guest list's own "you" row catches up to
        // it. Every other scenario's stand-in is never read by EventPopover
        // at all (see below), so its content doesn't matter.
        if (scenario === 'writes-back') {
          return {
            ...RESPOND_STUB_DETAIL,
            attendees: [
              { email: 'me@x.com', display_name: null, response_status: args.response,
                optional: false, is_self: true },
            ],
          };
        }
        // The resolved detail is deliberately never read by EventPopover: a
        // "this one" RSVP against a bare master leaves the backend's own
        // detail unchanged (see respond_to_event's doc comment), so the
        // popover shows the choice optimistically rather than trust this
        // return value. Any well-shaped stand-in satisfies its type.
        return RESPOND_STUB_DETAIL;
      }
      // The three write commands. None of them needs a hold, a forced failure
      // or a per-scenario answer: what every spec asserts on is the *arguments*
      // they were given — above all `occurrenceStartMs`, which must be the
      // clicked block's own `start_ms` and never `detail.start_ms` — and
      // `harness.calls` above already records those for every command, in
      // order. A second capture on `window` would be a second thing to keep in
      // step with it.
      case 'create_event':
        return CREATED_DETAIL;
      case 'update_event':
        // What the real command answers with: the freshly written detail. `App`
        // never reads it (it reloads the grid instead), so the unchanged
        // fixture is a truthful enough stand-in.
        return POPOVER_DETAILS[args.id] ?? CREATED_DETAIL;
      case 'delete_event_cmd':
        // Returns nothing, exactly as the Rust command does: the event the
        // popover was showing is gone, and reading it back would fail on the
        // runs that succeeded.
        return null;
      case 'sign_in':
        // Tauri rejects a `Result<_, String>` with the bare string, so the
        // app sees exactly the sentence Rust produced.
        if (scenario === 'no-config') return Promise.reject(NO_CONFIG_ERROR);
        if (scenario === 'sign-in-adds-account') {
          signedIn = true;
          status = { accounts: ['new@x.com'], last_sync_ms: null, demo: false };
          return 'new@x.com';
        }
        return 'me@x.com';
      default:
        throw new Error(`unstubbed command: ${cmd}`);
    }
  };

  (window as any).__TAURI_INTERNALS__ = {
    invoke,
    transformCallback(cb: (e: unknown) => void) {
      const id = nextId++;
      callbacks.set(id, cb);
      return id;
    },
  };
  (window as any).__harness = harness;
  return harness;
}

export { weekLabel };
