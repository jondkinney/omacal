import { invoke } from '@tauri-apps/api/core';

export type AppStatus = {
  accounts: string[];
  /** The subset of `accounts` whose Google sign-in is dead for good — the
   *  token endpoint said so, sync has stopped trying them, and signing in
   *  again is the only fix. Emails rather than a count, because "an account
   *  needs attention" with two connected is a guessing game. */
  needs_reauth: string[];
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
  /** Something failed. The one state that has to be noticed. */
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
 * `error` is `App`'s own error state, which is wider than sync: a write that
 * could not refresh sets it too, and `AppStatus` carries no sync-specific
 * failure field to narrow to. Keeping the wider signal is deliberate — it is
 * the only failure the header is given, and under-reporting one is worse than
 * occasionally over-reporting.
 *
 * **So the label does not claim it was a sync failure.** Saying "Last sync
 * failed" when a *write* could not refresh states as fact something the source
 * cannot support: over-reporting that a problem exists is defensible,
 * misreporting which problem it is is not. The dot says something is wrong and
 * the sentence says what, by carrying `App`'s own message rather than a
 * category invented here.
 *
 * If it ever proves noisy — a transient write failure leaving the light red
 * until the next operation — that is when a sync-specific field on `AppStatus`
 * earns its cost. Not before.
 */
export function syncLight(
  o: {
    connected: boolean; busy: boolean; error: string | null; reauth: boolean;
    lastSyncMs: number | null;
  },
  now: number,
): { state: SyncState; label: string } {
  if (o.busy) return { state: 'syncing', label: 'Syncing now' };
  if (o.error !== null) return { state: 'failed', label: `Something went wrong: ${o.error}` };
  // After `error`: an account parked behind a reconnect is a standing state,
  // and a fresh failure is news. Before `lastSyncMs`, or the light reads
  // "Synced 5 min ago" in green while an account it stopped syncing goes
  // quietly stale — the exact defect the `error` ordering above exists to
  // avoid, one row down. This is the sync-specific status field the comment
  // above said would one day earn its cost.
  if (o.reauth) return { state: 'failed', label: 'An account needs to be reconnected' };
  if (!o.connected) return { state: 'never', label: 'Not signed in' };
  if (o.lastSyncMs === null) return { state: 'never', label: 'Not synced yet' };
  return { state: 'synced', label: `Synced ${relativeTime(o.lastSyncMs, now)}` };
}

/**
 * The reconnect banner's sentence. Names the account, because with two
 * connected the user's next action differs by which one is dead; "is no
 * longer valid" rather than "expired", because a revoked grant and a
 * client-pair change both land here and neither is about time.
 */
export function reauthMessage(emails: string[]): string {
  return `Google sign-in for ${emails.join(', ')} is no longer valid. Reconnect to resume syncing.`;
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
