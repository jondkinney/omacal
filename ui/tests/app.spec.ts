// App.svelte — the wiring the component specs cannot reach: two Tauri event
// listeners, the week-loading effect, and what the user is shown when any of
// it goes wrong. Everything here runs against the real component with a
// stubbed IPC layer (tests/harness/tauri.ts).

import { test, expect, type Page } from '@playwright/test';
import {
  APP_MON, APP_NOW, weekLabel,
  APP_PRIMARY_CALENDAR_ID, APP_READER_CALENDAR_ID,
  APP_SERIES_DTSTART, APP_SERIES_OCCURRENCE,
  APP_ALLDAY_OCCURRENCE, APP_ALLDAY_SERIES_DTSTART,
} from './fixtures';
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

  // Plan 4 Task 5: Year and Big Year join the other three, replacing the
  // "two of them not yet built" spec this superseded — `disabled` slots are
  // gone for good (DoD: "no disabled slots").
  test('all five views are reachable', async ({ page }) => {
    await page.goto(app('connected'));
    await expect(page.locator('.vswitch button[disabled]')).toHaveCount(0);
    await page.keyboard.press('4');
    await expect(page.locator('.ymonth')).toHaveCount(12);
    await page.keyboard.press('5');
    await expect(page.locator('.rrow')).toHaveCount(14);
  });

  test('H and L step by a year in the year views', async ({ page }) => {
    await page.goto(app('connected'));
    // Same mount race every other spec in this file guards against — see
    // "number keys switch views" above: fired before `<svelte:window
    // onkeydown>` has attached, the keypress lands on nothing and is gone.
    await expect(page.locator('.vswitch button')).toHaveCount(5);
    await page.keyboard.press('4');
    const before = await page.locator('.ygrid').getAttribute('data-year');
    await page.keyboard.press('l');
    const after = await page.locator('.ygrid').getAttribute('data-year');
    expect(Number(after)).toBe(Number(before) + 1);
  });

  test('big year reaches this year and next, and no further back', async ({ page }) => {
    // Spec §4: it is a planning surface — what is coming, not what happened.
    await page.goto(app('connected'));
    await expect(page.locator('.vswitch button')).toHaveCount(5); // mount race — see above
    await page.keyboard.press('5');
    const opened = await page.locator('.ribbon').getAttribute('data-year');
    await page.keyboard.press('h');
    expect(await page.locator('.ribbon').getAttribute('data-year')).toBe(opened);
    await page.keyboard.press('l');
    expect(Number(await page.locator('.ribbon').getAttribute('data-year'))).toBe(Number(opened) + 1);
    await page.keyboard.press('l');
    expect(Number(await page.locator('.ribbon').getAttribute('data-year'))).toBe(Number(opened) + 1);
  });

  test('the arrows step a year too, and say so', async ({ page }) => {
    await page.goto(app('connected'));
    await expect(page.locator('.vswitch button')).toHaveCount(5); // mount race — see above
    await page.keyboard.press('4');
    await expect(page.getByRole('button', { name: 'Next year' })).toBeVisible();
    const before = await page.locator('.ygrid').getAttribute('data-year');
    await page.getByRole('button', { name: 'Next year' }).click();
    expect(Number(await page.locator('.ygrid').getAttribute('data-year'))).toBe(Number(before) + 1);
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

  test('the anchor date reaches Year view too, but never Big Year', async ({ page }) => {
    // Spec §5 says "every switch", and Year is a switch. `yearNum` is its own
    // counter seeded from the real clock, so Year used to open on the current
    // year no matter where the anchor stood — here, anchored in Aug 2026 under
    // a clock frozen to Jan 2024, it opened on 2024.
    await page.goto(app('connected'));
    await expect(page.locator('.vswitch button')).toHaveCount(5); // mount race — see above
    await page.keyboard.press('3');
    await page.locator('.mcell .num').nth(14).click(); // anchors on 10 Aug 2026
    await expect(page.locator('.col')).toHaveCount(1);

    await page.keyboard.press('4');
    await expect(page.locator('.ygrid')).toHaveAttribute('data-year', '2026');

    // Big Year is deliberately left out: spec §4 bounds it to the real current
    // year and the next, so an anchor in the past has nowhere to put it —
    // seeding it would either break the bound or drag the anchor forward past
    // it. The clock is frozen to Jan 2024, so that bound is 2024.
    await page.keyboard.press('5');
    await expect(page.locator('.ribbon')).toHaveAttribute('data-year', '2024');
  });

  // Whole-branch review, finding 1: the `<h1>` was derived from `weekStartMs`
  // in every view, so Day and Month — which render `anchorMs`'s own month —
  // were titled with the month the *Monday of that week* falls in. The two
  // disagree whenever the anchor's week began in the previous month, which is
  // two keystrokes away from any given day. 1 Feb 2024 is the shape: a
  // Thursday, in the week that started Mon 29 Jan.
  //
  // All three units in one spec, because the bug is precisely that they
  // shared one: Day and Month must read February, Week must still read
  // January for the same anchor.
  test('the title names the month of the unit actually on screen', async ({ page }) => {
    await page.goto(app('connected'));
    await expect(page.locator('.vswitch button')).toHaveCount(5); // see above
    await page.keyboard.press('1'); // Day view, anchored on Mon 29 Jan (APP_MON)
    await page.keyboard.press('l'); // 30 Jan
    await page.keyboard.press('l'); // 31 Jan
    await page.keyboard.press('l'); // 1 Feb — a Thursday, week still begins 29 Jan
    await expect(page.locator('h1')).toHaveText('February 2024');

    await page.keyboard.press('3'); // Month view: the February grid, titled for February
    await expect(page.locator('.mrow')).toHaveCount(6);
    await expect(page.locator('h1')).toHaveText('February 2024');

    // Week keeps its own rule — the month its Monday falls in — which is what
    // makes this anchor tell the three units apart at all.
    await page.keyboard.press('2');
    await expect(page.locator('h1')).toHaveText('January 2024');
  });

  // The same seam by mouse. `‹`/`›` used to step a week in *every* view while
  // announcing themselves as "Previous week"/"Next week" — in Month view that
  // moved the grid by a week, sometimes not changing the month at all and
  // sometimes crossing a boundary. One spec per unit, so each proves its own
  // motion rather than the first failure hiding the rest.
  test('the header arrows step a day in Day view, and say so', async ({ page }) => {
    await page.goto(app('connected'));
    await expect(page.locator('.vswitch button')).toHaveCount(5); // see above
    await page.keyboard.press('1');
    const next = page.getByRole('button', { name: 'Next day' });
    await expect(next).toBeVisible();
    await expect(page.locator('.col')).toHaveCount(1);

    const before = Number(await page.locator('.col').getAttribute('data-start-ms'));
    await next.click();
    // `toHaveAttribute` retries, so this waits for the re-fetch rather than
    // racing the click against it.
    await expect(page.locator('.col'))
      .toHaveAttribute('data-start-ms', String(before + 24 * 3600 * 1000)); // 30 Jan
    await page.getByRole('button', { name: 'Previous day' }).click();
    await expect(page.locator('.col')).toHaveAttribute('data-start-ms', String(before));
  });

  // Read off Day view's own column rather than the `<h1>`, so this pins the
  // *motion* independently of the title fix above: a week step from 29 Jan
  // lands on 5 Feb, which names February just as correctly as 29 Feb does.
  test('the header arrows step a month in Month view, and say so', async ({ page }) => {
    await page.goto(app('connected'));
    await expect(page.locator('.vswitch button')).toHaveCount(5); // see above
    await page.keyboard.press('3'); // Month view, anchored Mon 29 Jan (APP_MON)
    const next = page.getByRole('button', { name: 'Next month' });
    await expect(next).toBeVisible();
    await next.click();

    await page.keyboard.press('1');
    await expect(page.locator('.col'))
      .toHaveAttribute('data-start-ms', String(Date.UTC(2024, 1, 29))); // 29 Feb, not 5 Feb

    await page.keyboard.press('3');
    await page.getByRole('button', { name: 'Previous month' }).click();
    await page.keyboard.press('1');
    await expect(page.locator('.col'))
      .toHaveAttribute('data-start-ms', String(Date.UTC(2024, 0, 29))); // back to 29 Jan
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

  // Fix round 1, finding 2: the pinned "H and L step" spec above only ever
  // presses `2` first, so the day and month units were unexercised — the
  // reviewer changed the month branch to step by seven days (silently
  // behaving like week-stepping) and all 262 tests passed. Day and month
  // each get their own spec below.
  test('H and L step by a day when Day view is active', async ({ page }) => {
    await page.goto(app('connected'));
    await expect(page.locator('.vswitch button')).toHaveCount(5);
    await page.keyboard.press('1');
    const before = await page.locator('.col').first().getAttribute('data-start-ms');
    await page.keyboard.press('l');
    const after = await page.locator('.col').first().getAttribute('data-start-ms');
    expect(Number(after) - Number(before)).toBe(24 * 3600 * 1000);
  });

  // Fix round 1, finding 1: a bare `setMonth` overflows for a day-of-month
  // the target month doesn't have — Jan 31 `+1` used to land on Mar 3,
  // skipping February outright, and stayed wrong forever after (`H` from
  // there landed Feb 3, not back on Jan 31). This spec starts from the 31st
  // for exactly that reason — it is finding 1's regression spec and finding
  // 2's month-unit spec at once. 2024 is a leap year, so the correct,
  // clamped landing is Feb 29, not Feb 28 or Mar 3.
  test('H and L step by a month, clamped to the target month\'s last day', async ({ page }) => {
    await page.goto(app('connected'));
    await expect(page.locator('.vswitch button')).toHaveCount(5);
    await page.keyboard.press('1'); // Day view, anchored on 29 Jan (APP_MON)
    await page.keyboard.press('l'); // 30 Jan
    await page.keyboard.press('l'); // 31 Jan
    await page.keyboard.press('3'); // Month view; the anchor survives the switch (31 Jan)
    await page.keyboard.press('l'); // step +1 month from the 31st
    await page.keyboard.press('1'); // back to Day view to read the anchor off `.col`
    const shown = await page.locator('.col').getAttribute('data-start-ms');
    expect(Number(shown)).toBe(Date.UTC(2024, 1, 29)); // 29 Feb 2024 — clamped, not skipped to March
  });

  // Fix round 1, finding 4: `MonthGrid`'s own spec proves it *calls*
  // `onopen`; nothing proved App's `monthSelId`/`monthDetail`/`EventPopover`
  // wiring on the receiving end actually opens anything. The reviewer
  // replaced `onopen={openMonthEvent}` with a no-op and all 38 App specs
  // stayed green — the DoD's "clicking an event in any view opens the
  // popover" was unverified for Month.
  test('clicking an event in Month view opens the popover', async ({ page }) => {
    await page.goto(app('connected'));
    await expect(page.locator('.vswitch button')).toHaveCount(5);
    await page.keyboard.press('3');
    await page.locator('.mcell .timed').first().click();
    await expect(page.locator('.pop')).toBeVisible();
    await expect(page.locator('.pop h2')).toHaveText('Standup');
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

  // Task 10: creating, editing and deleting, wired into the views.
  //
  // Every one of these drives the real `App` against the stubbed IPC layer and
  // asserts on the *arguments* a write command was given, because that is where
  // this branch's one unrecoverable mistake lives: `occurrenceStartMs` has to be
  // the clicked block's own `start_ms` and never `detail.start_ms`, which for a
  // series is the master's DTSTART. Both values type-check, both read
  // correctly, and the wrong one silently rewrites or deletes occurrence #0 with
  // `sendUpdates=all` behind it. See `eventdetail.ts`.

  /** Every argument list the app has passed to `cmd`, in order. */
  const callsTo = (page: Page, cmd: string): Promise<any[]> =>
    page.evaluate(
      (c) => window.__harness.calls.filter((call) => call.cmd === c).map((call) => call.args),
      cmd,
    );

  /** The `writable` scenario, opened and settled: two editable events on
   *  Monday's column, and a calendar list that has actually landed.
   *
   *  Waiting for `.trigger` — Header's calendar picker button, which only
   *  exists once `calendars` is non-empty — is not decoration. `App` seeds a
   *  new event's calendar from that list, and `EventForm` snapshots the seed on
   *  mount; a form opened before `get_calendars` answered would carry `null`
   *  and refuse to save, intermittently. */
  const writable = async (page: Page) => {
    await page.goto(app('writable'));
    await expect(page.locator('.trigger')).toBeVisible();
    await expect(page.locator('.vswitch button')).toHaveCount(5); // mount race — see above
  };

  const block = (page: Page, title: string) => page.locator('.ev').filter({ hasText: title });
  /** An `AllDayBand` chip. A different element and a different component from
   *  `block` above — `commands::assemble_week` puts every `is_all_day` event in
   *  the band and never in a day column, so this is the *only* way to reach an
   *  all-day event's popover, and therefore its edit and delete. */
  const chip = (page: Page, title: string) => page.locator('.chip').filter({ hasText: title });
  const newForm = (page: Page) => page.getByRole('dialog', { name: 'New event' });
  const editForm = (page: Page) => page.getByRole('dialog', { name: 'Edit event' });
  const confirmPanel = (page: Page) => page.getByRole('dialog', { name: 'Delete event' });

  test('n opens the form on the anchor date', async ({ page }) => {
    // The anchor, not today. The clock is frozen to midday on Mon 29 Jan, so a
    // form built from `Date.now()` would open on the 29th — two `l`s away from
    // where the user is actually looking.
    await writable(page);
    await page.keyboard.press('1'); // Day view, anchored on Mon 29 Jan (APP_MON)
    await page.keyboard.press('l'); // 30 Jan
    await page.keyboard.press('l'); // 31 Jan
    await expect(page.locator('.col'))
      .toHaveAttribute('data-start-ms', String(Date.UTC(2024, 0, 31)));

    await page.keyboard.press('n');
    await expect(newForm(page)).toBeVisible();
    await expect(newForm(page).getByLabel('Date', { exact: true })).toHaveValue('2024-01-31');
    // The *time* still comes from the clock — the next half hour after midday.
    // Both halves asserted, because a form that took the whole instant from the
    // anchor would open at midnight and look almost right.
    await expect(newForm(page).getByLabel('Start', { exact: true })).toHaveValue('12:30');
  });

  test('clicking empty grid space opens the form at that time', async ({ page }) => {
    await writable(page);
    const col = page.locator('.col').first();
    await expect(col).toHaveAttribute('data-start-ms', String(APP_MON));
    const box = (await col.boundingBox())!;

    // 10:15 down the column: the middle of the 10:00 half hour, so this asserts
    // the snapping rule rather than pixel-exact hit testing — a dozen pixels of
    // slack either way still lands in the same slot. Well clear of both blocks,
    // which sit inside the column's top tenth.
    await col.click({ position: { x: box.width / 2, y: box.height * (10.25 / 24) } });

    await expect(newForm(page)).toBeVisible();
    await expect(newForm(page).getByLabel('Date', { exact: true })).toHaveValue('2024-01-29');
    await expect(newForm(page).getByLabel('Start', { exact: true })).toHaveValue('10:00');
    await expect(newForm(page).getByLabel('End', { exact: true })).toHaveValue('10:30');
  });

  // Year and Big Year keep their own counters and never touch `anchorMs`, so
  // `n` there used to open a form in the year the user had navigated *away*
  // from. That matters more in Year than anywhere else: it is the one view
  // with no empty grid space to click, so `n` is its only way to create, and a
  // substitute that lands in the wrong year is not a substitute. One spec per
  // view, because the two read different counters.
  test('n follows the year on screen in Year view, not the anchor', async ({ page }) => {
    await writable(page);
    await page.keyboard.press('4'); // Year, re-seeded from the anchor: 2024
    await expect(page.locator('.ygrid')).toHaveAttribute('data-year', '2024');
    await page.keyboard.press('l');
    await expect(page.locator('.ygrid')).toHaveAttribute('data-year', '2025');

    await page.keyboard.press('n');
    await expect(newForm(page)).toBeVisible();
    // The anchor's own month and day — 29 January — moved into the year on
    // screen, rather than 2024's.
    await expect(newForm(page).getByLabel('Date', { exact: true })).toHaveValue('2025-01-29');
  });

  test('n follows the year on screen in Big Year view too', async ({ page }) => {
    await writable(page);
    await page.keyboard.press('5');
    await expect(page.locator('.ribbon')).toHaveAttribute('data-year', '2024');
    await page.keyboard.press('l'); // its own bound is the real current year and next
    await expect(page.locator('.ribbon')).toHaveAttribute('data-year', '2025');

    await page.keyboard.press('n');
    await expect(newForm(page)).toBeVisible();
    await expect(newForm(page).getByLabel('Date', { exact: true })).toHaveValue('2025-01-29');
  });

  // Four entry points were delivered; the two above witness Day/Week and the
  // keyboard. These two are Month and Big Year, whose `oncreate` reaches `App`
  // through components `WeekGrid`'s specs never touch — `oncreate={() => {}}`
  // on both left the suite green. Fix round 1's finding 4 was this exact shape
  // for `onopen`, one control earlier.
  test('clicking a Month cell opens the form on that day', async ({ page }) => {
    await writable(page);
    await page.keyboard.press('3');
    // The grid's first cell, Mon 27 Jul 2026, which carries only its own
    // number — so the middle of it is genuinely empty space.
    await page.locator('.mcell').first().locator('.newhere').click();
    await expect(newForm(page)).toBeVisible();
    await expect(newForm(page).getByLabel('Date', { exact: true })).toHaveValue('2026-07-27');
  });

  test('clicking a Big Year day opens the form on that day', async ({ page }) => {
    await writable(page);
    await page.keyboard.press('5');
    // The ribbon is anchored on the Monday on or before 1 Jan of the year on
    // screen. The clock is frozen in Jan 2024 and 1 Jan 2024 *is* a Monday, so
    // the anchor is that day itself and row 0 column 10 is Thu 11 Jan.
    // Off-centre because a ribbon day is ~45px by ~15px with its own number in
    // the middle.
    await page.locator('.rrow').first().locator('.rday .newhere').nth(10)
      .click({ position: { x: 3, y: 3 } });
    await expect(newForm(page)).toBeVisible();
    await expect(newForm(page).getByLabel('Date', { exact: true })).toHaveValue('2024-01-11');
  });

  test('a new event lands on a calendar the user can write to', async ({ page }) => {
    // The list leads with a `reader` (a subscribed holiday calendar, the
    // ordinary case) and then a `writer`, with the user's own primary last, so
    // this id rejects both `calendars[0]` and "the first writable one".
    await writable(page);
    await page.keyboard.press('n');
    await expect(newForm(page)).toBeVisible();
    await newForm(page).getByLabel('Title', { exact: true }).fill('Lunch');
    await newForm(page).getByRole('button', { name: 'Create' }).click();
    await expect(newForm(page)).toHaveCount(0);

    const [args] = await callsTo(page, 'create_event');
    expect(args.calendarId).toBe(APP_PRIMARY_CALENDAR_ID);
    expect(args.calendarId).not.toBe(APP_READER_CALENDAR_ID);
    expect(args.fields.summary).toBe('Lunch');
  });

  /// The occurrence-identity property, at the top of the stack: the clicked
  /// block's own start_ms must reach the command, not detail.start_ms.
  test('editing an occurrence sends the clicked block start, not the series start', async ({ page }) => {
    await writable(page);
    const syncsBefore = (await callsTo(page, 'sync_now')).length;
    await block(page, 'Standup').click();
    await expect(page.getByRole('dialog', { name: 'Standup' })).toBeVisible();
    await page.getByRole('button', { name: 'Edit' }).click();

    // The form is anchored on the occurrence too — 1 Feb, not the series'
    // 29 Jan. That is the same value, one step earlier, and getting it wrong
    // here would make an untouched time read as a move of three days.
    await expect(editForm(page)).toBeVisible();
    await expect(editForm(page).getByLabel('Date', { exact: true })).toHaveValue('2024-02-01');

    await editForm(page).getByRole('button', { name: 'Save' }).click();
    await expect(editForm(page)).toHaveCount(0);

    const [args] = await callsTo(page, 'update_event');
    expect(args.occurrenceStartMs).toBe(APP_SERIES_OCCURRENCE);
    expect(args.occurrenceStartMs).not.toBe(APP_SERIES_DTSTART);
    // Task 9's anchoring invariant, asserted at the only place both values
    // exist together: an untouched time means these two are equal *exactly*,
    // because the Rust side reads any difference between them as a move.
    expect(args.fields.when.startMs).toBe(args.occurrenceStartMs);

    // And the edit is followed by a sync, not just a local re-read. This is
    // the *edit* path's own witness: it shares `refreshAfterWrite` with the
    // delete path, but a shared function is not a shared assertion — replacing
    // this call site alone with `reload()` left the whole suite green, and the
    // consequence is the one `updateEvent`'s doc comment names, that a `this`
    // edit against a bare master is not written back locally and the popover
    // goes on showing the old title for up to a sync interval.
    await expect
      .poll(async () => (await callsTo(page, 'sync_now')).length)
      .toBeGreaterThan(syncsBefore);
  });

  test('delete asks for confirmation and names the event', async ({ page }) => {
    await writable(page);
    await block(page, 'Standup').click();
    await page.getByRole('button', { name: 'Delete' }).click();

    await expect(confirmPanel(page)).toBeVisible();
    await expect(confirmPanel(page).locator('h2')).toContainText('Standup');
    // Asked, not done. Nothing may have gone to Google at the moment the
    // question is on screen — that is the whole of what "confirm" means, and
    // there is no undo behind it.
    expect(await callsTo(page, 'delete_event_cmd')).toEqual([]);
  });

  test('a non-recurring event offers no scope choice', async ({ page }) => {
    // Without this, the three-scope spec below passes on a confirmation that
    // always shows three radios, whatever it was given.
    await writable(page);
    await block(page, 'Board prep').click();
    await page.getByRole('button', { name: 'Delete' }).click();
    await expect(confirmPanel(page)).toBeVisible();
    await expect(confirmPanel(page).getByRole('radio')).toHaveCount(0);
  });

  test('a recurring event offers all three scopes', async ({ page }) => {
    await writable(page);
    await block(page, 'Standup').click();
    await page.getByRole('button', { name: 'Delete' }).click();

    const panel = confirmPanel(page);
    await expect(panel.getByRole('radio')).toHaveCount(3);
    await expect(panel.getByRole('radio', { name: 'This event' })).toBeChecked();
    await expect(panel.getByRole('radio', { name: 'This and following' })).toHaveCount(1);
    await expect(panel.getByRole('radio', { name: 'All events' })).toHaveCount(1);
  });

  test('deleting an occurrence sends the clicked block start, and syncs after it', async ({ page }) => {
    await writable(page);
    const syncsBefore = (await callsTo(page, 'sync_now')).length;

    await block(page, 'Standup').click();
    await page.getByRole('button', { name: 'Delete' }).click();
    await confirmPanel(page).getByRole('button', { name: 'Delete' }).click();
    await expect(confirmPanel(page)).toHaveCount(0);

    const [args] = await callsTo(page, 'delete_event_cmd');
    expect(args.scope).toBe('this');
    expect(args.occurrenceStartMs).toBe(APP_SERIES_OCCURRENCE);
    expect(args.occurrenceStartMs).not.toBe(APP_SERIES_DTSTART);

    // A `this` delete against a bare master patches a Google resource this app
    // has no row for, so the backend correctly skips its local write-back and
    // the block stays on screen. Re-reading the database cannot find that out;
    // only a sync can, which is why the refresh after a write is a sync.
    await expect
      .poll(async () => (await callsTo(page, 'sync_now')).length)
      .toBeGreaterThan(syncsBefore);
  });

  // Whole-branch review, finding 3: Task 10 wired `AllDayBand` chips into the
  // same popover -> edit/delete route as `EventBlock` (`WeekGrid.svelte`,
  // `onopen={openPopover}`), and nothing witnessed it end to end — searching
  // this file for `all_day`/`all-day`/`AllDay` returned nothing, and
  // `AllDayBand`'s own specs are screenshots and chip corners, which never
  // click a chip at all.
  //
  // It matters more than an ordinary gap. An all-day event has no `EventBlock`
  // to fall back on, so a chip is its whole surface; and the all-day edit path
  // is exactly where this branch's *pinned* date-boundary defect lives
  // (`eventform.spec.ts`), so the one route that reaches a known-shipped bug
  // was the one route with no App-level witness. Both specs assert on
  // `occurrenceStartMs`, for the reason the block above gives: it must be the
  // chip's own day and never `detail.start_ms`, which here is the all-day
  // series' DTSTART two days earlier.

  test('editing from an all-day chip sends the chip\'s own day', async ({ page }) => {
    await writable(page);
    await chip(page, 'Diwali').click();
    await expect(page.getByRole('dialog', { name: 'Diwali' })).toBeVisible();
    await page.getByRole('button', { name: 'Edit' }).click();

    // The form is anchored on the clicked day too — Wed 31 Jan, not the
    // series' Mon 29 Jan — which is the same value one step earlier, and the
    // step at which getting it wrong turns an untouched date into a two-day
    // move of every following occurrence. `First day`, not `Date`: an all-day
    // event's form renders the date pair under its own labels (see
    // `EventForm.svelte`), which is itself a small proof that the all-day
    // branch is the one on screen.
    await expect(editForm(page)).toBeVisible();
    await expect(editForm(page).getByLabel('First day', { exact: true }))
      .toHaveValue('2024-01-31');

    await editForm(page).getByRole('button', { name: 'Save' }).click();
    await expect(editForm(page)).toHaveCount(0);

    const [args] = await callsTo(page, 'update_event');
    expect(args.occurrenceStartMs).toBe(APP_ALLDAY_OCCURRENCE);
    expect(args.occurrenceStartMs).not.toBe(APP_ALLDAY_SERIES_DTSTART);
  });

  test('deleting from an all-day chip sends the chip\'s own day', async ({ page }) => {
    await writable(page);
    await chip(page, 'Diwali').click();
    await expect(page.getByRole('dialog', { name: 'Diwali' })).toBeVisible();
    await page.getByRole('button', { name: 'Delete' }).click();

    await expect(confirmPanel(page)).toBeVisible();
    await confirmPanel(page).getByRole('button', { name: 'Delete' }).click();
    await expect(confirmPanel(page)).toHaveCount(0);

    const [args] = await callsTo(page, 'delete_event_cmd');
    // `'this'` is the confirmation's own default, and it is the scope that
    // makes the day matter most: aimed at the DTSTART it removes the series'
    // *first* occurrence rather than the one whose chip was clicked, and mails
    // every guest about it.
    expect(args.scope).toBe('this');
    expect(args.occurrenceStartMs).toBe(APP_ALLDAY_OCCURRENCE);
    expect(args.occurrenceStartMs).not.toBe(APP_ALLDAY_SERIES_DTSTART);
  });

  test('cancelling the confirmation deletes nothing', async ({ page }) => {
    await writable(page);
    await block(page, 'Standup').click();
    await page.getByRole('button', { name: 'Delete' }).click();
    await confirmPanel(page).getByRole('button', { name: 'Cancel' }).click();
    await expect(confirmPanel(page)).toHaveCount(0);
    expect(await callsTo(page, 'delete_event_cmd')).toEqual([]);
  });

  test('editing from the Month view popover reaches the form too', async ({ page }) => {
    // Month and Big Year do not go through `WeekGrid`: `App` renders a second
    // `EventPopover` of its own for them, so its Edit/Delete wiring is
    // separate code that every spec above leaves untouched. This is finding 4
    // of Fix round 1 again, one control further on — that one caught
    // `onopen={() => {}}` on this exact popover with 38 App specs still green.
    await writable(page);
    await page.keyboard.press('3');
    await page.locator('.mcell .timed').first().click();
    await expect(page.getByRole('dialog', { name: 'Standup' })).toBeVisible();
    await page.getByRole('button', { name: 'Edit' }).click();

    await expect(editForm(page)).toBeVisible();
    // And the popover it was opened from is gone, rather than left behind its
    // own scrim under the form — two stacked modals, each claiming
    // `aria-modal`, with the one underneath unreachable.
    await expect(page.getByRole('dialog', { name: 'Standup' })).toHaveCount(0);
    // The clicked block's own instant, carried through `App`'s own
    // `gridSelStart`/`gridSelEnd`. `busyDayMonth` builds its Standup at
    // `BUSY_DAY_START_MS + 9h`, and that constant is carried over verbatim
    // from the Rust suite rather than being this file's own UTC midnight for
    // Mon 10 Aug (see its comment in fixtures.ts) — it is 06:00 UTC, so under
    // the project's `timezoneId: 'UTC'` the form reads 15:00.
    //
    // The *date* is what makes this discriminating: this event's detail is a
    // weekly master whose DTSTART is Mon **3** Aug, so `App` handing the form
    // `gridDetail.start_ms` — the Plan 2 defect, in its second instance —
    // shows 2026-08-03 here. The End field binds `gridSelEnd` the same way.
    await expect(editForm(page).getByLabel('Date', { exact: true })).toHaveValue('2026-08-10');
    await expect(editForm(page).getByLabel('Start', { exact: true })).toHaveValue('15:00');
    await expect(editForm(page).getByLabel('End', { exact: true })).toHaveValue('15:30');
  });

  test('the views do not move under an open form', async ({ page }) => {
    // Focus is moved to a control *outside* the panel first, and that is the
    // whole spec. `isTypingTarget` already drops a view key aimed at an
    // `<input>` or at anything inside `.pop`, and the form takes focus on its
    // title field on mount — so a spec that presses `3` straight after opening
    // the form passes whether or not the modal guard exists. (Written that way
    // first; removing the guard left it green.)
    //
    // Reachable without contrivance: the panel has no focus trap, so tabbing
    // past its Create button lands on whatever is behind the scrim, and a bare
    // `3` from there would switch the view under an open form.
    await writable(page);
    await page.keyboard.press('n');
    await expect(newForm(page)).toBeVisible();

    await page.getByRole('button', { name: 'Today' }).focus();
    await page.keyboard.press('3');
    await expect(page.locator('.mrow')).toHaveCount(0);
    await expect(newForm(page)).toBeVisible();
  });

  test('edit and delete are hidden when can_edit is false', async ({ page }) => {
    // The default scenario's one event takes `detail()`'s own `can_edit: false`
    // — which is why that default is `false` (see fixtures.ts): a fixture list
    // that were editable throughout would satisfy the "shown" specs above by
    // itself, and a `can_edit` check nobody wrote would look implemented.
    await page.goto(app());
    await page.locator('.ev').click();
    await expect(page.locator('.pop')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Edit' })).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Delete' })).toHaveCount(0);
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
