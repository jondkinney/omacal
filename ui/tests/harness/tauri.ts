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

import type { WeekPayload } from '../../src/lib/api';
import type { AppStatus } from '../../src/lib/status';
import type { Calendar } from '../../src/lib/calendars';
import type { EventDetail } from '../../src/lib/eventdetail';
import {
  labelledWeek, weekLabel, APP_FIVE_MIN_AGO, POPOVER_DETAILS, POPOVER_REFRESHED_DETAIL,
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
  /** Park the *next* `event_detail` or `refresh_event` call for this exact
   *  event id instead of answering it — what a `WeekGrid` spec uses to
   *  drive the supersession guard (open one block while another's detail is
   *  still loading) and the after-paint refresh (control when it lands). */
  holdNextEventCall(cmd: 'event_detail' | 'refresh_event', id: number): void;
  /** Answer the parked call for this command and id, then let its `.then`
   *  chain — including `WeekGrid`'s own `detail = fresh` — actually run. */
  releaseEventCall(cmd: 'event_detail' | 'refresh_event', id: number, value: EventDetail): Promise<void>;
  /** Make the next `event_detail` or `refresh_event` call for this exact id
   *  reject — what a `WeekGrid` spec uses to drive the load-failure-closes
   *  path (`event_detail`) without disturbing every other id's fixture. */
  failNextEventCall(cmd: 'event_detail' | 'refresh_event', id: number, message: string): void;
  /** Reject the *parked* call for this command and id — the counterpart to
   *  `releaseEventCall`, for proving a *late failure* for a superseded
   *  click does not close a popover that opened successfully after it.
   *  `failNextEventCall` alone cannot drive that: it rejects immediately,
   *  before a second click could ever land. */
  rejectEventCall(cmd: 'event_detail' | 'refresh_event', id: number, message: string): Promise<void>;
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
  organizer_email: null, self_response: null, can_respond: true, attendees: [],
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
type EventCmd = 'event_detail' | 'refresh_event';

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

/** Resolves against `POPOVER_DETAILS`/`POPOVER_REFRESHED_DETAIL` unless a
 *  spec armed a failure or a hold for this exact command and id. */
function eventCallResult(cmd: EventCmd, id: number): Promise<EventDetail> {
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
  // Both commands default to the same, unchanged `POPOVER_DETAILS` entry —
  // the ordinary case, where nothing moved since the popover opened.
  // `POPOVER_REFRESHED_DETAIL` (id 50) only ever reaches a spec through an
  // explicit `releaseEventCall('refresh_event', 50, POPOVER_REFRESHED_DETAIL)`,
  // never through this default path.
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
    color_hex: '#5b8def', selected: true, sync_enabled: true, is_primary: true },
  { id: 2, account_id: 1, account_email: 'new@x.com', summary: 'Holidays',
    color_hex: '#e2a03f', selected: true, sync_enabled: true, is_primary: false },
];

function getWeek(weekStartMs: number): Promise<WeekPayload> {
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
  return Promise.resolve(labelledWeek(weekStartMs));
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
        return getWeek(args.weekStartMs);
      // App's own effect fetches calendars alongside status on mount. None of
      // the App specs exercise the popover, and Header only renders it once
      // `calendars.length > 0`, so an empty list keeps every existing
      // assertion undisturbed. CalendarPopover specs never take this path —
      // they mount the component directly with fixture props instead.
      case 'get_calendars':
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
      case 'respond_to_event':
        // Exposed so a spec can assert on exactly what `EventPopover` sent —
        // in particular, that the fourth argument is the clicked block's own
        // `start_ms` and never `detail.start_ms` (the task brief's trap).
        (window as any).__lastRespondCall = args;
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
