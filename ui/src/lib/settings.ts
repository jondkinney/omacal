import { invoke } from '@tauri-apps/api/core';

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

/** Stores the filmstrip toggle. Nothing is refused: unlike the sync interval
 *  there is no value of a boolean the app has to protect anything from. */
export const setListMode = (on: boolean) =>
  invoke<AppSettings>('set_list_mode', { on });

/** Minutes, as the General tab shows them. Stored in milliseconds because
 *  that is what `sync_loop` compares against a clock. */
export const minutesOf = (ms: number): number => Math.round(ms / 60_000);
export const msOfMinutes = (min: number): number => Math.round(min * 60_000);
