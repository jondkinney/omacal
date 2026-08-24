import { invoke } from '@tauri-apps/api/core';

import type { TimeFormat } from './timefmt';
import type { WeekStartDay } from './weekstart';

/**
 * The preferences the settings modal edits.
 *
 * Mirrors `settings::AppSettings` field for field. `minSyncIntervalMs` is
 * published by the backend rather than written here on purpose: the form has
 * to say what the minimum is in order to refuse a smaller one with a reason,
 * and a second copy of that number in TypeScript is one that drifts from the
 * `sync_loop::MIN_INTERVAL_MS` actually enforced.
 */
export type AppSettings = {
  /** As **stored**, not as clamped. The loop clamps on the way out, because a
   *  row edited by hand with `sqlite3` — until now the only way to set this,
   *  documented in both platform guides — never passed through the command
   *  that refuses. A form showing the clamped value would silently disagree
   *  with the database it is editing. */
  syncIntervalMs: number;
  notificationsEnabled: boolean;
  minSyncIntervalMs: number;
  /** Whether Day, Week and Month draw as a list rather than a grid (filmstrip
   *  spec §4). No settings tab shows it — the control is the `▦`/`☰` beside the
   *  view switcher — but it is a preference and is stored beside the others,
   *  which is what makes it survive a restart. */
  listMode: boolean;
  /** Minutes-before for the fallback reminders (fallback spec §3): what fires
   *  for a timed event that follows its calendar's defaults when the calendar
   *  has none. Popup by construction — omacal never sends email. */
  fallbackReminderMinutes: number[];
  /** The calendar a new event lands on unless the user picks another, or
   *  `null` for the old rule — primary, else first writable. Stored
   *  unvalidated; `offerableCalendarId` guards staleness at every use. */
  defaultCalendarId: number | null;
  /** Whether the app draws `13:30` or `1:30 PM`. Read by `timefmt.ts` through
   *  the `clock.svelte.ts` rune rather than as a prop — six components print a
   *  time and none of them owns the preference. */
  timeFormat: TimeFormat;
  /** The day a week begins on. Read by the grids through the
   *  `weekstartstore.svelte.ts` rune, for the same reason `timeFormat` is. */
  weekStart: WeekStartDay;
  /** Whether the system tray icon is shown. On by default — the tray is where
   *  Quit lives. Turning it off is for setups where something else carries
   *  those actions, like Omarchy 4's bar widget. */
  trayIcon: boolean;
  /** Whether the day headers carry the forecast — an icon and the high. On
   *  by default; the hint under the toggle names the sources (Open-Meteo,
   *  the Omarchy widget's location or the IP), because this is the one
   *  network destination beyond the calendar providers. */
  weatherEnabled: boolean;
  /** The IANA zone every time in the app reads in, or `null` for the
   *  system's. Applied by exporting `TZ` before the webview starts, which is
   *  why changing it restarts omacal — the JS engine and libc both capture
   *  the zone at process start and offer no runtime swap. */
  displayTimezone: string | null;
};

export const getSettings = () => invoke<AppSettings>('get_settings');

/**
 * Stores a new sync interval and answers with the settings as they now are.
 *
 * **Rejects below the floor rather than clamping**, and the rejection is the
 * point: a value accepted and then quietly changed is worse than one turned
 * down. The form refuses first, so this is the second of two guards rather
 * than the only one — but it is the one that holds if the form ever forgets.
 */
export const setSyncInterval = (ms: number) =>
  invoke<AppSettings>('set_sync_interval', { ms });

export const setNotificationsEnabled = (on: boolean) =>
  invoke<AppSettings>('set_notifications_enabled', { on });

/** Stores the tray-icon preference; the backend also applies it to the
 *  running tray immediately, so the icon reacts to the click. */
export const setTrayIcon = (on: boolean) =>
  invoke<AppSettings>('set_tray_icon', { on });

/** Stores the weather preference; a turn-on also fetches now, backend-side,
 *  so the headers change while the modal is still open. */
export const setWeatherEnabled = (on: boolean) =>
  invoke<AppSettings>('set_weather_enabled', { on });

/** Stores the clock format. Nothing is refused: `settings::TimeFormat` has two
 *  variants and the select offers both, so there is no third value to turn
 *  down — see the note on `set_time_format`. */
export const setTimeFormat = (format: TimeFormat) =>
  invoke<AppSettings>('set_time_format', { format });

/** Stores the day a week begins on. Nothing is refused: the select offers
 *  exactly the three variants `settings::WeekStart` has. */
export const setWeekStart = (start: WeekStartDay) =>
  invoke<AppSettings>('set_week_start', { start });

/** Stores the filmstrip toggle. Nothing is refused: unlike the sync interval
 *  there is no value of a boolean the app has to protect anything from. */
export const setListMode = (on: boolean) =>
  invoke<AppSettings>('set_list_mode', { on });

/** Stores the fallback reminder rows. The backend refuses out-of-bounds
 *  values with the limit named (fallback spec §3); `[]` is accepted and is
 *  the feature turned off. */
export const setFallbackReminders = (minutes: number[]) =>
  invoke<AppSettings>('set_fallback_reminders', { minutes });

/** Stores the default calendar for new events; `null` clears the choice. */
/** Every zone the picker may offer — jiff's copy of the IANA database, the
 *  same authority the setter validates against. */
export const listTimezones = () => invoke<string[]>('list_timezones');

/**
 * Stores the display zone and **restarts omacal** to apply it; `null`
 * returns to the system zone. The reply arrives just before the restart, so
 * the form has one breath to say what is about to happen.
 */
export const setDisplayTimezone = (tz: string | null) =>
  invoke<void>('set_display_timezone', { tz });

export const setDefaultCalendar = (id: number | null) =>
  invoke<AppSettings>('set_default_calendar', { id });

/** Minutes, as the General tab shows them. Stored in milliseconds because
 *  that is what `sync_loop` compares against a clock. */
export const minutesOf = (ms: number): number => Math.round(ms / 60_000);
export const msOfMinutes = (min: number): number => Math.round(min * 60_000);
