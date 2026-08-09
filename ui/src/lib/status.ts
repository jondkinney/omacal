import { invoke } from '@tauri-apps/api/core';

export type AppStatus = {
  accounts: string[];
  last_sync_ms: number | null;
  demo: boolean;
  /** True when the window's controls are drawn over the webview instead of in
   *  a strip above it, so the header has to leave room for them — macOS with
   *  `titleBarStyle: "Overlay"`, and nothing else.
   *
   *  This is the only thing `ui/src` knows about the platform it runs on, and
   *  it deliberately arrives as a *property of the window* rather than as an
   *  OS name: "leave room at the left" is what the header needs to decide, and
   *  the OS is only one of the two things that decides it (see
   *  `status::controls_overlay_content` for the other). It rides on
   *  `get_status` because that is already fetched on mount; the alternative
   *  was `@tauri-apps/plugin-os`, a dependency for one boolean. */
  overlay_titlebar: boolean;
};

export const getStatus = () => invoke<AppStatus>('get_status');
export const signIn = () => invoke<string>('sign_in');
export const syncNow = () => invoke<number>('sync_now');

/**
 * What the header's status light is showing.
 *
 * Four states, because four is what a person can tell apart at 8px without a
 * label — spec §2's table, named.
 */
export type SyncState =
  /** A sync is in flight. Active. */
  | 'syncing'
  /** Something failed and the calendar may be stale. The one state that has to
   *  be noticed. */
  | 'failed'
  /** Nothing to be stale: signed out, or signed in and nothing fetched yet. */
  | 'never'
  /** The normal case, and the one that must be quiet enough to ignore. */
  | 'synced';

/**
 * The status light's whole answer: which state, and the sentence that says so.
 *
 * **One call for both, and that is the design rather than a convenience.**
 * Spec §2 requires that a colour is never the only signal, and the way to
 * guarantee it is not to remember to write the words — it is to make the
 * colour and the words the same answer. The component derives its class *and*
 * its `aria-label`/`title` from this one result, so a dot showing one thing
 * while announcing another is not a state this code can reach.
 *
 * The order the facts are read in is the only real decision here, and each
 * step is a case where two things are true at once:
 *
 *   1. **`busy` first.** A sync in flight outranks the failure it is retrying;
 *      otherwise the light stays red through the very moment it is being fixed.
 *   2. **`error` before `lastSyncMs`.** `lastSyncMs` is the last *successful*
 *      sync and survives a failed one, so reading it first reports "Synced
 *      5 min ago" while the calendar quietly goes stale — the exact defect a
 *      status light exists to make visible.
 *   3. **Signed out is not a failure.** Nothing has gone wrong; there is
 *      nothing to sync. A red dot on a fresh install is the app telling a new
 *      user it is broken.
 *
 * `error` is `App`'s own error state, which is wider than sync — a write that
 * could not refresh sets it too. That is imprecise and deliberately so for
 * now: it is the only failure signal the header is given, the banner beneath
 * carries the words, and a light that under-reports failure would be worse
 * than one that occasionally over-reports it.
 */
export function syncLight(
  o: { connected: boolean; busy: boolean; error: string | null; lastSyncMs: number | null },
  now: number,
): { state: SyncState; label: string } {
  if (o.busy) return { state: 'syncing', label: 'Syncing now' };
  if (o.error !== null) return { state: 'failed', label: 'Last sync failed' };
  if (!o.connected) return { state: 'never', label: 'Not signed in' };
  if (o.lastSyncMs === null) return { state: 'never', label: 'Not synced yet' };
  return { state: 'synced', label: `Synced ${relativeTime(o.lastSyncMs, now)}` };
}

/** "just now" / "4 min ago" / "2 h ago" — deliberately coarse. */
export function relativeTime(ms: number | null, now = Date.now()): string {
  if (ms === null) return 'never';
  const s = Math.max(0, Math.floor((now - ms) / 1000));
  if (s < 60) return 'just now';
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
  return `${Math.floor(s / 86400)} d ago`;
}
