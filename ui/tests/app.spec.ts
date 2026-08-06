// App.svelte — the wiring the component specs cannot reach: two Tauri event
// listeners, the week-loading effect, and what the user is shown when any of
// it goes wrong. Everything here runs against the real component with a
// stubbed IPC layer (tests/harness/tauri.ts).

import { test, expect } from '@playwright/test';
import { APP_MON, APP_NOW, weekLabel } from './fixtures';
import { NO_CONFIG_ERROR } from './harness/tauri';

const WEEK = 7 * 24 * 3_600_000;
/** The week the app opens on, and the one a click on › moves to. */
const W1 = APP_MON;
const W2 = APP_MON + WEEK;

const app = (fixture = 'default') => `/tests/harness/index.html?c=App&f=${fixture}`;

test.describe('App', () => {
  test.beforeEach(async ({ page }) => {
    // `weekStart(new Date())` decides which week the app opens on, so the
    // clock has to be frozen before the page loads, not after.
    await page.clock.setFixedTime(APP_NOW);
  });

  test('loads the current week and reports the last sync', async ({ page }) => {
    await page.goto(app());
    await expect(page.locator('h1')).toHaveText('January 2024');
    await expect(page.locator('.ev b')).toHaveText(weekLabel(W1));
    await expect(page.locator('.synced')).toHaveText('Synced 5 min ago');
    await expect(page.locator('.err')).toHaveCount(0);
  });

  test('navigating forward loads the next week', async ({ page }) => {
    await page.goto(app());
    await expect(page.locator('.ev b')).toHaveText(weekLabel(W1));
    await page.getByRole('button', { name: 'Next week' }).click();
    await expect(page.locator('h1')).toHaveText('February 2024');
    await expect(page.locator('.ev b')).toHaveText(weekLabel(W2));
  });

  // D3. A background reload is issued for the week on screen, the user moves
  // on before it answers, and the answer arrives last. Without a stamp on each
  // request the late answer wins and paints the old week under the new week's
  // header — which is `$derived` from `weekStartMs` and has already moved —
  // and nothing short of navigating again ever puts it right.
  test('a slow background reload never repaints a week the user has left', async ({ page }) => {
    await page.goto(app());
    await expect(page.locator('.ev b')).toHaveText(weekLabel(W1));

    // Park the reload that `sync-finished` is about to trigger for W1.
    await page.evaluate((w) => window.__harness.hold(w), W1);
    await page.evaluate(() => window.__harness.emit('sync-finished', { upserted: 3 }));
    await expect.poll(() => page.evaluate(() => window.__harness.held())).toBe(1);

    // The user moves to next week while that request is still in flight.
    await page.getByRole('button', { name: 'Next week' }).click();
    await expect(page.locator('h1')).toHaveText('February 2024');
    await expect(page.locator('.ev b')).toHaveText(weekLabel(W2));

    // Now the stale answer lands. It must be dropped, not painted.
    await page.evaluate((w) => window.__harness.release(w), W1);
    await expect(page.locator('.ev b')).toHaveText(weekLabel(W2));
    await expect(page.locator('h1')).toHaveText('February 2024');
  });

  // D2. The header's "Synced N ago" is computed from the last *successful*
  // sync, so it is structurally incapable of reporting that sync is broken.
  test('a failed background sync says so', async ({ page }) => {
    await page.goto(app());
    await expect(page.locator('.err')).toHaveCount(0);

    await page.evaluate(() =>
      window.__harness.emit('sync-failed', {
        message: 'Sync failed — omacal could not reach Google. It will keep trying.',
      }),
    );
    await expect(page.locator('.err')).toContainText('could not reach Google');
  });

  // D2, the other half: the label has to keep counting on its own. It used to
  // recompute only when `status` changed — that is, only when a sync
  // succeeded — so it froze at its last value exactly when sync stopped.
  test('the synced label keeps counting while nothing happens', async ({ page }) => {
    await page.clock.install({ time: APP_NOW });
    await page.goto(app());
    await expect(page.locator('.synced')).toHaveText('Synced 5 min ago');

    await page.clock.fastForward('10:00');
    await expect(page.locator('.synced')).toHaveText('Synced 15 min ago');
  });

  // D6. This reload used to sit outside any try/catch, so a failure here was
  // an unhandled rejection in the console and nothing at all on screen.
  test('a failed background reload surfaces instead of vanishing', async ({ page }) => {
    await page.goto(app());
    await expect(page.locator('.ev b')).toHaveText(weekLabel(W1));

    await page.evaluate(() => window.__harness.failNextWeek('database is locked'));
    await page.evaluate(() => window.__harness.emit('sync-finished', { upserted: 1 }));
    await expect(page.locator('.err')).toContainText('database is locked');
  });

  // D5. The likeliest first-run failure, and the half that matters — "Create
  // it with client_id and client_secret" — is the half a 320px ellipsised
  // line threw away.
  test('the first-run config error is readable in full', async ({ page }) => {
    await page.goto(app('no-config'));
    await page.getByRole('button', { name: 'Connect Google Calendar' }).click();

    const err = page.locator('.err');
    await expect(err).toContainText('Create it with client_id and client_secret');

    // Not clipped horizontally, and taller than the single line it used to be
    // squeezed into.
    const clipped = await err.evaluate((el) => el.scrollWidth > el.clientWidth + 1);
    expect(clipped, 'the error text is being cut off horizontally').toBe(false);
    const box = await err.boundingBox();
    expect(box!.height).toBeGreaterThan(24);
  });

  test('the error clears when the user moves to another week', async ({ page }) => {
    await page.goto(app('no-config'));
    await page.getByRole('button', { name: 'Connect Google Calendar' }).click();
    await expect(page.locator('.err')).toContainText(NO_CONFIG_ERROR);

    await page.getByRole('button', { name: 'Next week' }).click();
    await expect(page.locator('.err')).toHaveCount(0);
  });

  // Task 6: the calendar popover is Header's own concern, but the reload
  // that feeds it is App's. Without it, a second account's calendars stay
  // invisible until the app is relaunched — a second account you cannot see
  // is the same as no second account.
  test('signing in reloads the calendar list', async ({ page }) => {
    await page.goto(app());
    const calendarCalls = () =>
      page.evaluate(() =>
        window.__harness.calls.filter((c) => c.cmd === 'get_calendars').length,
      );
    // Wait for the mount effect's own `get_calendars` to land before taking
    // the baseline — otherwise that call can race the click below and land
    // during the "after" window instead, inflating the count for the wrong
    // reason and passing even if sign-in never reloads anything.
    await expect.poll(calendarCalls).toBeGreaterThan(0);
    const before = await calendarCalls();

    await page.getByRole('button', { name: 'Add account' }).click();
    await expect.poll(calendarCalls).toBeGreaterThan(before);
  });

  // Task 7: the picker opens after every sign-in — first account or fifth —
  // so a freshly imported set of calendars, all switched on by default, is
  // never left syncing behind the user's back without them having seen it.
  test('signing in opens the picker with the new calendars in it', async ({ page }) => {
    await page.goto(app('sign-in-adds-account'));
    await page.getByRole('button', { name: /Connect|Add account/ }).click();
    await expect(page.locator('.panel')).toBeVisible();
    await expect(page.locator('.acct')).toHaveCount(1);
  });

  // Fix round 1 (Task 7), finding 2: `open` is bound two levels deep —
  // `App` to `Header` to `CalendarPopover` — and every existing spec signs
  // in exactly once, so a one-way binding on either leg is invisible: the
  // panel would still be visible right after that first sign-in even if
  // closing it never made its way back up to `App`'s own `pickerOpen`. Only
  // a second sign-in, after the first close, can catch `pickerOpen` stuck
  // true and unable to reopen the (by-then-closed) child.
  test('closing the picker then signing in again reopens it', async ({ page }) => {
    await page.goto(app('sign-in-adds-account'));
    await page.getByRole('button', { name: /Connect|Add account/ }).click();
    await expect(page.locator('.panel')).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(page.locator('.panel')).toHaveCount(0);

    await page.getByRole('button', { name: 'Add account' }).click();
    await expect(page.locator('.panel')).toBeVisible();
  });

  // Same round trip, via the click-away close path rather than Escape —
  // both are ways `CalendarPopover` sets its own `open` to false, and the
  // reviewer who found this confirmed both propagate up correctly.
  test('clicking away then signing in again reopens it', async ({ page }) => {
    await page.goto(app('sign-in-adds-account'));
    await page.getByRole('button', { name: /Connect|Add account/ }).click();
    await expect(page.locator('.panel')).toBeVisible();
    await page.locator('.scrim').click();
    await expect(page.locator('.panel')).toHaveCount(0);

    await page.getByRole('button', { name: 'Add account' }).click();
    await expect(page.locator('.panel')).toBeVisible();
  });

  // Task 5: the switcher, the keyboard, and the shared anchor date. `.col` is
  // WeekGrid's own day-column class (see `WeekGrid.svelte`; the brief's draft
  // used `.daycol` as a placeholder, same correction Task 2 already made for
  // its own two specs).
  test('the switcher offers five views, two of them not yet built', async ({ page }) => {
    await page.goto(app('connected'));
    await expect(page.locator('.vswitch button')).toHaveCount(5);
    // `exact: true`: Playwright's default (non-exact) name match is a
    // substring test, and "Year" is a substring of "Big Year" too — without
    // it this resolves to both buttons and throws a strict-mode violation
    // before ever reaching the assertion. "Big Year" has no such collision
    // the other way, so it needs no adjustment.
    await expect(page.getByRole('button', { name: 'Year', exact: true })).toBeDisabled();
    await expect(page.getByRole('button', { name: 'Big Year' })).toBeDisabled();
  });

  test('number keys switch views', async ({ page }) => {
    await page.goto(app('connected'));
    // `page.keyboard.press` is a one-shot dispatch, not an auto-waiting
    // locator action — pressed before `<svelte:window onkeydown>` has
    // actually attached (mount.svelte.ts imports `App.svelte` via a dynamic
    // `import()`, which can still be in flight when `goto` resolves), the
    // keydown lands on a window with no listener yet and is gone for good;
    // nothing about it is retried. Waiting for the switcher itself — the
    // thing that same mount attaches — is what makes the keypress land on a
    // window that's actually listening.
    await expect(page.locator('.vswitch button')).toHaveCount(5);
    await page.keyboard.press('3');
    await expect(page.locator('.mrow')).toHaveCount(6);
    await page.keyboard.press('1');
    await expect(page.locator('.col')).toHaveCount(1);
  });

  test('the anchor date survives a view switch', async ({ page }) => {
    // Spec §5: switching Month -> Day lands on the day you were looking at,
    // not on today. This is what makes "+N more" and the day-number click
    // work as handoffs rather than jumps.
    await page.goto(app('connected'));
    await expect(page.locator('.vswitch button')).toHaveCount(5); // see above
    await page.keyboard.press('3');
    await page.locator('.mcell .num').nth(14).click();
    await expect(page.locator('.col')).toHaveCount(1);
    const shown = await page.locator('.col').getAttribute('data-start-ms');
    expect(Number(shown)).toBe(1786341600000); // the day that was clicked
  });

  test('H and L step by the current view\'s unit', async ({ page }) => {
    await page.goto(app('connected'));
    await expect(page.locator('.vswitch button')).toHaveCount(5); // see above
    await page.keyboard.press('2');
    const before = await page.locator('.col').first().getAttribute('data-start-ms');
    await page.keyboard.press('l');
    const after = await page.locator('.col').first().getAttribute('data-start-ms');
    expect(Number(after) - Number(before)).toBe(7 * 24 * 3600 * 1000);
  });

  test('T returns to today', async ({ page }) => {
    await page.goto(app('connected'));
    const col = page.locator('.col').first();
    // Capture where the app opened rather than inventing a global to remember
    // it — whatever "today" is for the fixture clock, two steps forward and T
    // must land back on exactly this value. `getAttribute` is itself an
    // auto-waiting locator read, so it already doubles as the mount-stability
    // wait the other specs above take explicitly.
    const opened = await col.getAttribute('data-start-ms');
    await page.keyboard.press('l');
    await page.keyboard.press('l');
    expect(await col.getAttribute('data-start-ms')).not.toBe(opened);
    await page.keyboard.press('t');
    expect(await col.getAttribute('data-start-ms')).toBe(opened);
  });

  // Own spec, guarding the keyboard trap (not in the brief's five): the event
  // popover has RSVP buttons and a description, and a stray `3` while one of
  // those buttons has focus must not switch views out from under it.
  test('typing keys are ignored while the event popover has focus', async ({ page }) => {
    await page.goto(app('connected'));
    await page.locator('.ev').click();
    await expect(page.locator('.pop')).toBeVisible();
    await page.getByRole('button', { name: 'Yes' }).focus();
    await page.keyboard.press('3');
    await expect(page.locator('.mrow')).toHaveCount(0);
    await expect(page.locator('.pop')).toBeVisible();
  });

  test('a theme-changed event repaints without a reload', async ({ page }) => {
    await page.goto(app());
    await page.evaluate(() =>
      window.__harness.emit('theme-changed', {
        bg: '#fdfdfb', surface: '#ffffff', text: '#1a1a1a',
        muted: '#6b6b70', accent: '#c04a2b', is_dark: false,
      }),
    );
    await expect
      .poll(() =>
        page.evaluate(() =>
          document.documentElement.style.getPropertyValue('--accent').trim(),
        ),
      )
      .toBe('#c04a2b');
  });
});
