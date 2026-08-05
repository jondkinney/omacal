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
import { labelledWeek, weekLabel, APP_FIVE_MIN_AGO } from '../fixtures';

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
  /** Every command the app has invoked, in order. */
  calls: { cmd: string; args: unknown }[];
};

/** What `set_calendar_sync(id, false)` reports removing, absent a forced failure. */
export const CALENDAR_SYNC_REMOVED = 143;

const listeners = new Map<string, Set<(e: unknown) => void>>();
const callbacks = new Map<number, (e: unknown) => void>();
const hold = new Set<number>();
const parked = new Map<number, Deferred>();

let nextId = 1;
let failWeekOnce: string | null = null;
let failCalendarOnce: { cmd: string; message: string } | null = null;

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
};

/** Resolves normally unless a spec armed a failure for this exact command. */
function calendarResult<T>(cmd: string, ok: T): Promise<T> {
  if (failCalendarOnce?.cmd === cmd) {
    const { message } = failCalendarOnce;
    failCalendarOnce = null;
    return Promise.reject(message);
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
      return { accounts: [], last_sync_ms: null, demo: false };
    default:
      return { accounts: ['me@x.com'], last_sync_ms: APP_FIVE_MIN_AGO, demo: false };
  }
}

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
  const status = statusFor(scenario);

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
        return [] as Calendar[];
      case 'set_calendar_selected':
        return calendarResult(cmd, undefined);
      case 'set_calendar_sync':
        return calendarResult(cmd, CALENDAR_SYNC_REMOVED);
      case 'sync_now':
        return 0;
      case 'sign_in':
        // Tauri rejects a `Result<_, String>` with the bare string, so the
        // app sees exactly the sentence Rust produced.
        if (scenario === 'no-config') return Promise.reject(NO_CONFIG_ERROR);
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
