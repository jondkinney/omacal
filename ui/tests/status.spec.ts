import { test, expect } from '@playwright/test';
import { syncLight, relativeTime, tzChangeMessage, type SyncState } from '../src/lib/status';

/**
 * The status light's whole answer, as a function.
 *
 * Pure and exercised directly, because the thing worth testing is the
 * **mapping** — which fact wins when two are true at once — and a rendered
 * header can only reach the combinations a fixture happens to describe.
 *
 * The colour is not tested here and is not tested anywhere: a test that reads
 * a computed colour proves the stylesheet resolved a variable, which is a
 * fact about CSS rather than about sync. What is asserted is the **name**, and
 * the reason that is enough is structural — the component derives both the
 * class and the `aria-label` from this one call, so a colour that disagrees
 * with the words is not a state this code can reach.
 */
const NOW = Date.UTC(2026, 7, 9, 12, 0);

/** The healthy case, as arguments. Each test names only what it changes.
 *  Typed rather than inferred: `Partial<typeof ok>` on an inferred literal
 *  would type `lastSyncMs` as `number | undefined`, and the case that matters
 *  most — never synced — is exactly the one that passes `null`. */
type Args = Parameters<typeof syncLight>[0];
const ok: Args = {
  connected: true,
  busy: false,
  error: null,
  reauth: false,
  lastSyncMs: NOW - 5 * 60_000,
};

const light = (over: Partial<Args> = {}) => syncLight({ ...ok, ...over }, NOW);

test.describe('the status light', () => {
  const cases: Array<{ over: Partial<Args>; state: SyncState; label: string; why: string }> = [
    { over: {}, state: 'synced', label: 'Synced 5 min ago',
      why: 'the normal case, and it still says when' },
    { over: { busy: true }, state: 'syncing', label: 'Syncing now',
      why: 'something is happening right now' },
    { over: { error: 'network unreachable' }, state: 'failed',
      label: 'Something went wrong: network unreachable',
      why: 'the one state that has to be noticed — and it names what, not a category' },
    { over: { reauth: true }, state: 'failed', label: 'An account needs to be reconnected',
      why: 'an account the backend stopped syncing is a failure with a fix' },
    { over: { connected: false }, state: 'never', label: 'Not signed in',
      why: 'nothing to sync, which is not a failure' },
    { over: { lastSyncMs: null }, state: 'never', label: 'Not synced yet',
      why: 'signed in, nothing fetched yet — same colour, different sentence' },
  ];

  for (const c of cases) {
    test(`${c.state}: ${c.why}`, () => {
      const got = light(c.over);
      expect(got.state).toBe(c.state);
      expect(got.label).toBe(c.label);
    });
  }

  /**
   * **The order the facts are read in, which is the only part with a real
   * decision in it.** Each of these is two states at once, and the answer says
   * which one a person needs to see.
   */
  test('a sync in flight outranks the failure it is retrying', () => {
    // Otherwise the light stays red through the retry that is fixing it, and
    // the one moment the user is watching says nothing is happening.
    expect(light({ busy: true, error: 'network unreachable' }).state).toBe('syncing');
  });

  test('a reconnect outranks the last successful sync', () => {
    // `lastSyncMs` survives the account going dead — it is the last success —
    // so a light reading it first says "Synced 5 min ago" in green while an
    // account sync has stopped trying goes quietly stale. Same defect as the
    // `error` ordering below, one row down.
    expect(light({ reauth: true }).state).toBe('failed');
  });

  test('a fresh failure outranks the standing reconnect', () => {
    // Both are red; the words differ. A reconnect is a standing state and a
    // failure is news — and `error` carries its own message, which would be
    // lost the other way round.
    expect(light({ reauth: true, error: 'network unreachable' }).label)
      .toBe('Something went wrong: network unreachable');
  });

  test('a failure outranks a successful sync in the past', () => {
    // `lastSyncMs` is still set after a failed sync — the last *successful* one
    // — so a version reading it first reports "Synced 5 min ago" in green while
    // the calendar is quietly going stale. That is the defect this light exists
    // to make visible.
    expect(light({ error: 'network unreachable' }).state).toBe('failed');
  });

  /**
   * **The label says what went wrong, not which subsystem.**
   *
   * `error` is `App`'s own — a write that could not refresh sets it too, and
   * `AppStatus` carries no sync-specific failure to narrow to. Claiming "Last
   * sync failed" there states as fact something the source cannot support:
   * over-reporting that a problem exists is defensible, misreporting which
   * problem it is is not.
   */
  test('a failure carries the actual message rather than a category', () => {
    expect(light({ error: 'the token was revoked' }).label)
      .toBe('Something went wrong: the token was revoked');
    // And it never names sync, because it does not know that it was sync.
    expect(light({ error: 'the token was revoked' }).label).not.toContain('sync');
  });

  test('being signed out is not a failure', () => {
    // Nothing has gone wrong; there is simply nothing to sync. A red dot on a
    // fresh install would be the app telling a new user it is broken.
    expect(light({ connected: false }).state).toBe('never');
    expect(light({ connected: false }).state).not.toBe('failed');
  });

  /** And the words never contradict the colour, because there is one answer:
   *  every state below returns a non-empty sentence, so no state can be a
   *  colour with nothing behind it. */
  test('every state carries a sentence, not only a colour', () => {
    for (const c of cases) {
      const got = light(c.over);
      expect(got.label.length, `${got.state} has no words`).toBeGreaterThan(0);
    }
  });
});

test.describe('relativeTime', () => {
  // Kept honest here rather than only through the header that renders it: the
  // light's label is built from it, so a change to its wording changes what
  // the accessible name says.
  test('reads coarsely, and says never for nothing', () => {
    expect(relativeTime(null, NOW)).toBe('never');
    expect(relativeTime(NOW - 30_000, NOW)).toBe('just now');
    expect(relativeTime(NOW - 5 * 60_000, NOW)).toBe('5 min ago');
    expect(relativeTime(NOW - 3 * 3_600_000, NOW)).toBe('3 h ago');
    expect(relativeTime(NOW - 2 * 86_400_000, NOW)).toBe('2 d ago');
  });
});

test.describe('tzChangeMessage', () => {
  // Both zones by name: the difference is the damage, and the sentence has to
  // say what every hour on the grid currently means, not just where the
  // machine went.
  test('names where the machine went and what is still on screen', () => {
    expect(tzChangeMessage('Asia/Kolkata', 'Europe/Sofia')).toBe(
      'This machine moved to Asia/Kolkata, but times are still shown in Europe/Sofia. Restart OmaCal to catch up.'
    );
  });
});
