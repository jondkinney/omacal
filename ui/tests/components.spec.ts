import { test, expect } from '@playwright/test';
import {
  FIXED_NOW, POPOVER_DETAILS, POPOVER_REFRESHED_DETAIL, popoverWeekWithResponse, YEAR_2026_NOW,
} from './fixtures';
import { CALENDAR_SYNC_REMOVED } from './harness/tauri';

const show = (c: string, f: string) => `/tests/harness/index.html?c=${c}&f=${f}`;

test.describe('WeekGrid', () => {
  test('renders an empty week', async ({ page }) => {
    await page.goto(show('WeekGrid', 'empty'));
    await expect(page.locator('.col')).toHaveCount(7);
    await expect(page).toHaveScreenshot('weekgrid-empty.png');
  });

  test('renders overlaps side by side', async ({ page }) => {
    await page.goto(show('WeekGrid', 'populated'));
    // Thursday's two identical-time meetings must not sit on top of each other.
    const blocks = page.locator('.col').nth(3).locator('.ev');
    await expect(blocks).toHaveCount(2);
    const a = await blocks.nth(0).boundingBox();
    const b = await blocks.nth(1).boundingBox();
    expect(a && b).toBeTruthy();
    expect(a!.x + a!.width).toBeLessThanOrEqual(b!.x + 1);
    await expect(page).toHaveScreenshot('weekgrid-populated.png');
  });

  test('a one-day grid renders a single column', async ({ page }) => {
    await page.goto('/tests/harness/index.html?c=WeekGrid&f=single-day');
    await expect(page.locator('.col')).toHaveCount(1);
  });

  // Whole-branch review, finding 4: `.col` counting alone never noticed that
  // `--cols` had gone back to a hard-coded 7 — the single column still
  // existed, it was just drawn in the first seventh of the grid (172px of
  // 1248) with six-sevenths of the screen blank beside it. `--cols` exists to
  // produce this geometry, so the geometry is what has to be asserted.
  test('a one-day grid gives the day the whole width', async ({ page }) => {
    await page.goto('/tests/harness/index.html?c=WeekGrid&f=single-day');
    const col = page.locator('.col');
    await expect(col).toHaveCount(1);
    const colBox = (await col.boundingBox())!;
    const gridBox = (await page.getByTestId('week-body').boundingBox())!;
    // Everything but the 44px hour gutter. 0.9 sits well clear of both
    // outcomes — ~0.965 correct, ~0.138 with the column stuck at one seventh.
    expect(colBox.width / gridBox.width).toBeGreaterThan(0.9);
  });

  test('overlapping events fan out fully in a one-day grid', async ({ page }) => {
    // Spec §4: Day always fans out rather than stacking into columns — there is
    // width to spare and no reason to compress.
    await page.goto('/tests/harness/index.html?c=WeekGrid&f=single-day-overlap');
    const blocks = page.locator('.col .ev');
    await expect(blocks).toHaveCount(2);
    const a = await blocks.nth(0).boundingBox();
    const b = await blocks.nth(1).boundingBox();
    expect(a!.x).not.toBe(b!.x);
    // 80 is not a day/week boundary — a day-wide half renders around 600px,
    // a week-wide one around 82px. It is only a sanity floor against "fanned
    // but squeezed to nothing"; do not tune it as if it separated the two.
    expect(Math.min(a!.width, b!.width)).toBeGreaterThan(80);
  });
});

// Fix round 1: `EventPopover`'s own specs mount it standalone against a
// fixture that already carries the right `occurrenceStartMs` — they can only
// prove the popover honours its own prop, never that `WeekGrid` computed
// that prop correctly from the clicked block in the first place. These
// specs click a real block in a real `WeekGrid` instead, exercising
// `openPopover` end to end: the trap itself, the supersession guard, the
// load-failure close, the after-paint refresh, and the optimistic restyle.
test.describe('WeekGrid popover flow', () => {
  const show = (f: string) => `/tests/harness/index.html?c=WeekGrid&f=${f}`;

  test("responding sends the clicked block's own start, not the series DTSTART", async ({ page }) => {
    await page.goto(show('popover'));
    await page.getByRole('button', { name: 'Standup' }).click();
    await expect(page.locator('.pop')).toBeVisible();
    await page.getByRole('button', { name: 'No' }).click();
    const call = await page.evaluate(() => (window as any).__lastRespondCall);
    // POPOVER_DETAILS[42].start_ms (the series DTSTART) vs the block's own
    // start_ms (POPOVER_RECURRING's, the fourth occurrence) — see fixtures.ts.
    expect(call.occurrenceStartMs).toBe(POPOVER_DETAILS[42].start_ms + 3 * 24 * 3_600_000);
    expect(call.occurrenceStartMs).not.toBe(POPOVER_DETAILS[42].start_ms);
  });

  test('a successful response restyles the clicked block without a refetch', async ({ page }) => {
    await page.goto(show('popover'));
    const block = page.getByRole('button', { name: 'Standup' });
    await expect(block).toHaveClass(/needsAction/);
    await block.click();
    await page.getByRole('button', { name: 'No' }).click();
    await expect(block).toHaveClass(/declined/);
  });

  test('closing the popover mid-RSVP still restyles the block once the response lands', async ({ page }) => {
    // `detail` inside EventPopover is a live prop, not a snapshot: closing
    // the popover (the scrim, here) while `respondToEvent` is still in
    // flight sets WeekGrid's own `detail` to null, and `respond()`'s
    // closure keeps running regardless — exactly the case `onresponded`'s
    // restyle exists to still get right.
    await page.goto(show('popover'));
    await page.evaluate(() => window.__harness.holdNextEventCall('respond_to_event', 42));
    const block = page.getByRole('button', { name: 'Standup' });
    await block.click();
    await expect(page.locator('.pop')).toBeVisible();
    await page.getByRole('button', { name: 'No' }).click(); // parked mid-flight
    await page.locator('.scrim').click(); // close before the response lands
    await expect(page.locator('.pop')).toHaveCount(0);
    await page.evaluate(
      (detail) => window.__harness.releaseEventCall('respond_to_event', 42, detail),
      POPOVER_DETAILS[42],
    );
    await expect(block).toHaveClass(/declined/);
  });

  test('the popover updates in place once the after-paint refresh lands', async ({ page }) => {
    await page.goto(show('popover'));
    await page.evaluate(() => window.__harness.holdNextEventCall('refresh_event', 50));
    await page.getByRole('button', { name: 'Sync' }).click();
    await expect(page.locator('.loc')).toHaveText('Room A');
    await page.evaluate(
      (detail) => window.__harness.releaseEventCall('refresh_event', 50, detail),
      POPOVER_REFRESHED_DETAIL,
    );
    await expect(page.locator('.loc')).toHaveText('Room B');
  });

  test('a failed load never shows an empty popover', async ({ page }) => {
    await page.goto(show('popover'));
    await page.evaluate(() => window.__harness.failNextEventCall('event_detail', 60, 'offline'));
    await page.getByRole('button', { name: 'Event A' }).click();
    await expect(page.locator('.pop')).toHaveCount(0);
  });

  test('a late failure for a superseded click does not close a popover that opened after it', async ({ page }) => {
    // The failure counterpart to the "stale detail" spec below: block A's
    // load is still in flight when B is opened and succeeds; A's load then
    // fails. Without the `isSelected` guard on the catch branch, that late
    // failure would call `closePopover()` unconditionally and tear down
    // B's already-open, already-successful popover.
    await page.goto(show('popover'));
    await page.evaluate(() => window.__harness.holdNextEventCall('event_detail', 60));
    await page.getByRole('button', { name: 'Event A' }).click(); // parked
    await page.getByRole('button', { name: 'Event B' }).click(); // succeeds
    await expect(page.locator('.pop h2')).toHaveText('Event B');
    await page.evaluate(() => window.__harness.rejectEventCall('event_detail', 60, 'offline'));
    await expect(page.locator('.pop')).toBeVisible();
    await expect(page.locator('.pop h2')).toHaveText('Event B');
  });

  test('a stale detail arriving after a second block was opened is ignored', async ({ page }) => {
    await page.goto(show('popover'));
    await page.evaluate(() => window.__harness.holdNextEventCall('event_detail', 60));
    await page.getByRole('button', { name: 'Event A' }).click(); // parked
    await page.getByRole('button', { name: 'Event B' }).click(); // answers immediately
    await expect(page.locator('.pop h2')).toHaveText('Event B');
    await page.evaluate(
      (detail) => window.__harness.releaseEventCall('event_detail', 60, detail),
      POPOVER_DETAILS[60],
    );
    // Still B — the late arrival for A must not clobber what's on screen.
    await expect(page.locator('.pop h2')).toHaveText('Event B');
  });

  test('an override survives a payload that still disagrees with the baseline it was recorded against', async ({ page }) => {
    await page.goto(show('popover'));
    const block = page.getByRole('button', { name: 'Standup' });
    await block.click();
    await page.getByRole('button', { name: 'No' }).click();
    await expect(block).toHaveClass(/declined/);

    // A fresh sync lands (what App.svelte's loadWeek does after a real
    // sync — replaces `week` wholesale), but Standup's own response in it
    // still reads 'needsAction', exactly what it was when the override was
    // recorded. Nothing has actually caught up yet, so the override must
    // still win.
    await page.evaluate((week) => (window as any).__setWeek(week), popoverWeekWithResponse(42, 'needsAction'));
    await expect(block).toHaveClass(/declined/);
  });

  test('an override clears once the payload moves off the baseline it was recorded against', async ({ page }) => {
    await page.goto(show('popover'));
    const block = page.getByRole('button', { name: 'Standup' });
    await block.click();
    await page.getByRole('button', { name: 'No' }).click();
    await expect(block).toHaveClass(/declined/);

    // A fresh sync lands with a response that differs from the baseline —
    // accepted from another device, or anything else. Without eviction, the
    // override would keep masking every future payload for the rest of the
    // session; the payload must win once it actually disagrees.
    await page.evaluate((week) => (window as any).__setWeek(week), popoverWeekWithResponse(42, 'accepted'));
    await expect(block).toHaveClass(/accepted/);
    await expect(block).not.toHaveClass(/declined/);
  });

  test('a late failure for one occurrence does not close a popover open for a different occurrence sharing the same id', async ({ page }) => {
    // Coverage gap: every other fixture here has at most one occurrence per
    // store row id, so dropping `start_ms` from `isSelected` (leaving only
    // `id`) still passed every other spec in this file. Two occurrences of
    // one series, sharing an id, close it.
    await page.goto(show('popover-two-occurrences'));
    await page.evaluate(() => window.__harness.holdNextEventCall('event_detail', 70));
    await page.getByRole('button', { name: 'Daily sync 1' }).click(); // parked
    await page.getByRole('button', { name: 'Daily sync 2' }).click(); // same id, different start_ms — succeeds
    await expect(page.locator('.pop')).toBeVisible();
    await page.evaluate(() => window.__harness.rejectEventCall('event_detail', 70, 'offline'));
    // Occurrence 1's late failure must not close occurrence 2's popover —
    // it would, if `isSelected` compared `id` alone.
    await expect(page.locator('.pop')).toBeVisible();
  });

  // `commands::assemble_week` routes every `is_all_day` event into
  // `all_day_events` and never into a day column, so a band chip is the only
  // representation one ever gets. The chips were plain `<div>`s carrying a
  // `title` and nothing else — no click, no role, no tab stop — which meant
  // an all-day off-site with a guest list simply could not be opened.
  test('an all-day chip opens the popover with its guest list', async ({ page }) => {
    await page.goto(show('popover-all-day'));
    await page.getByRole('button', { name: 'Team off-site' }).click();
    await expect(page.locator('.pop h2')).toHaveText('Team off-site');
    const guests = page.locator('.pop .guest');
    await expect(guests).toHaveCount(2);
    // `.who` rather than the row: the row also carries the status glyph and a
    // visually-hidden status word, and asserting on the whole row would break
    // every time either changes while proving nothing extra about who is here.
    await expect(guests.nth(0).locator('.who')).toHaveText('Ana');
    await expect(guests.nth(1).locator('.who')).toContainText('(you)');
  });

  test('an all-day chip opens from the keyboard, not only the mouse', async ({ page }) => {
    // Free with a real `<button>`, and only with one: a `<div role="button">`
    // would need its own keydown handler, and a bare `<div>` — what this was
    // — is not reachable by Tab at all. Following `EventBlock`'s element
    // choice is what makes this pass with no key handling in `AllDayBand`.
    await page.goto(show('popover-all-day'));
    await page.getByRole('button', { name: 'Team off-site' }).focus();
    await page.keyboard.press('Enter');
    await expect(page.locator('.pop h2')).toHaveText('Team off-site');
  });

  // The one that matters. All-day occurrences are contiguous by construction
  // — each ends exactly where the next begins — which is the shape the
  // backend's instance lookup resolves most delicately, and why the bracket
  // fix and the body-provenance fix had to land before this path was opened
  // at all. If the chip passed anything but its own `start_ms`, the RSVP
  // would land on the series' first day with `sendUpdates=all`.
  test("an all-day recurring RSVP sends the clicked day, not the series start", async ({ page }) => {
    await page.goto(show('popover-all-day'));
    await page.getByRole('button', { name: 'Diwali' }).click();
    await expect(page.locator('.pop')).toBeVisible();
    await page.getByRole('button', { name: 'No' }).click();
    const call = await page.evaluate(() => (window as any).__lastRespondCall);
    // POPOVER_DETAILS[81].start_ms is the series DTSTART; the clicked chip is
    // the third day of the series — see fixtures.ts.
    expect(call.occurrenceStartMs).toBe(POPOVER_DETAILS[81].start_ms + 2 * 24 * 3_600_000);
    expect(call.occurrenceStartMs).not.toBe(POPOVER_DETAILS[81].start_ms);
    expect(call.scope).toBe('this');
  });
});

test.describe('EventBlock duration ladder', () => {
  test('15 minutes shows title only', async ({ page }) => {
    await page.goto(show('EventBlock', 'ladder-15'));
    await expect(page.locator('.ev b')).toHaveText('Sync w/ Ivan');
    await expect(page.locator('.ev em')).toHaveCount(0);
  });

  test('60 minutes adds one meta line', async ({ page }) => {
    await page.goto(show('EventBlock', 'ladder-60'));
    await expect(page.locator('.ev em')).toHaveCount(1);
  });

  test('120 minutes gives the time its own line', async ({ page }) => {
    await page.goto(show('EventBlock', 'ladder-120'));
    await expect(page.locator('.ev em')).toHaveCount(2);
  });
});

test.describe('EventBlock RSVP states at 15 minutes', () => {
  for (const state of ['accepted', 'needsAction', 'tentative', 'declined']) {
    test(`${state} is visually distinct`, async ({ page }) => {
      await page.goto(show('EventBlock', `rsvp-${state}-15`));
      await expect(page.locator('.ev')).toHaveClass(new RegExp(state));
      await expect(page.locator('#app')).toHaveScreenshot(`rsvp-${state}-15.png`);
    });
  }

  test('an unanswered invite carries its marker', async ({ page }) => {
    await page.goto(show('EventBlock', 'rsvp-needsAction-15'));
    await expect(page.locator('.ev .rs')).toHaveText('?');
  });
});

// A hovered block widens over its neighbours. The resting fills are near-
// transparent by design, so if hover does not also make the block opaque, the
// covered block's title reads straight through it — two labels on top of each
// other, which is worse than the squeeze hover exists to relieve.
test.describe('EventBlock hover occludes what it covers', () => {
  // Chromium reports color-mix() results as `color(srgb r g b / a)` and plain
  // colours as `rgb(...)`/`rgba(...)`. Anything else THROWS rather than
  // defaulting to opaque: a parser that assumes the good case on an
  // unrecognised format silently passes the exact test it exists to fail.
  const alpha = (css: string): number => {
    const fn = css.match(/^color\([^/)]*(?:\/\s*([0-9.]+))?\)$/);
    if (fn) return fn[1] === undefined ? 1 : parseFloat(fn[1]);
    const rgb = css.match(/^rgba?\(([^)]+)\)$/);
    if (rgb) {
      const parts = rgb[1].split(/[,\s/]+/).filter(Boolean).map(parseFloat);
      return parts.length < 4 ? 1 : parts[3];
    }
    throw new Error(`unrecognised colour format, cannot assess opacity: ${css}`);
  };

  for (const state of ['accepted', 'needsAction', 'tentative', 'declined']) {
    // Blocks overlap constantly, and every state must occlude the one behind it
    // — at rest, not only under the cursor. A translucent block lets the covered
    // event's title read through it and its rounded corners poke past, which is
    // what "ugly corners" turned out to be.
    test(`${state} is opaque at rest and on hover`, async ({ page }) => {
      await page.goto(show('EventBlock', `rsvp-${state}-15`));
      const ev = page.locator('.ev');

      const read = async () => ({
        bg: await ev.evaluate((el) => getComputedStyle(el).backgroundColor),
        op: await ev.evaluate((el) => parseFloat(getComputedStyle(el).opacity)),
      });

      const rest = await read();
      expect(alpha(rest.bg), `resting ${state} background must be opaque, got ${rest.bg}`).toBe(1);
      // Element opacity makes a block see-through regardless of its background,
      // so fading must be done with colours instead.
      expect(rest.op, `resting ${state} element opacity must be 1`).toBe(1);

      await ev.hover();
      const hov = await read();
      expect(alpha(hov.bg), `hovered ${state} background must be opaque, got ${hov.bg}`).toBe(1);
      expect(hov.op, `hovered ${state} element opacity must be 1`).toBe(1);
    });
  }

  // Element opacity used to do the fading. Colour has to carry it now, or
  // removing the transparency would quietly turn "declined" into "accepted".
  test('a declined block still reads as declined', async ({ page }) => {
    await page.goto(show('EventBlock', 'rsvp-declined-15'));
    await expect(page.locator('.ev b')).toHaveCSS('text-decoration-line', 'line-through');
    const declined = await page.locator('.ev').evaluate((el) => getComputedStyle(el).color);

    await page.goto(show('EventBlock', 'rsvp-accepted-15'));
    const accepted = await page.locator('.ev').evaluate((el) => getComputedStyle(el).color);

    expect(declined, 'declined must not render the same text colour as accepted')
      .not.toBe(accepted);
  });
});

test.describe('AllDayBand', () => {
  test('spans the right columns and flags a continuation', async ({ page }) => {
    await page.goto(show('AllDayBand', 'populated'));
    const chips = page.locator('.chip');
    await expect(chips).toHaveCount(2);
    // The span arriving from last week gets the flat dashed edge.
    await expect(chips.nth(1)).toHaveClass(/cl/);
    await expect(chips.nth(1)).toContainText('‹');
    await expect(page.locator('#app')).toHaveScreenshot('allday-populated.png');
  });

  test('reports overflow', async ({ page }) => {
    await page.goto(show('AllDayBand', 'overflow'));
    await expect(page.locator('.more')).toHaveText('+2 more');
  });

  test('renders nothing when there is nothing to show', async ({ page }) => {
    await page.goto(show('AllDayBand', 'empty'));
    await expect(page.locator('.band')).toHaveCount(0);
  });
});

// The chip's colour spine is a border on one side only, meeting a
// border-radius. That is the exact geometry behind the artifact
// `EventBlock.svelte` documents: WebKit derives each corner's curve from the
// two border widths meeting there, and in macOS WKWebView the corners away
// from the border rendered square. `EventBlock` removed the cause by
// replacing its spine with an inset shadow; a chip cannot, because `.cl` has
// to draw that spine *dashed*, which no shadow can do. So the chip keeps the
// shape and this guards it instead.
//
// Per chip and at zero tolerance, neither of which is incidental. The band's
// own `allday-populated.png` is 1280x42 under the config's
// `maxDiffPixelRatio: 0.01` — about 537 pixels of slack, against roughly 3-4
// pixels per corner. That snapshot would not notice this artifact returning;
// it has ~5% of its budget to spare on it. A chip-sized frame at
// `maxDiffPixels: 0` has none.
//
// `threshold: 0` is load-bearing, and not obviously so. `maxDiffPixels: 0`
// alone does nothing here: `threshold` is the *per-pixel* tolerance pixelmatch
// applies before a pixel counts as differing at all, and at its default of 0.2
// a squared-off corner is invisible. The chip's fill is
// `color-mix(… 16%, transparent)`, so a corner pixel flipping from page
// background to chip fill moves (23,23,26) to about (55,45,32) — a YIQ delta
// of ~314 against the 1409 that threshold 0.2 permits. Being nearly
// transparent is exactly what makes this artifact cheap to miss. Even
// threshold 0.1 (1409 -> 352) still ignores it; anything at or above ~0.095
// does. Verified by mutation, not by reading: squaring the two corners away
// from the border passes at the default and fails at 0.
//
// Zero costs nothing in stability here — the four baselines below were
// produced by a different element type on a different run and match byte for
// byte — because the frame holds no antialiased text edges that move between
// runs on a fixed platform.
//
// These four baselines were generated from the pre-change `<div>` markup
// (`git show 6d278b8:ui/src/lib/AllDayBand.svelte`) and are committed
// unmodified: the `<button>` this became reproduces them pixel for pixel in
// both engines, which is the evidence that the swap cost nothing. From here
// they guard against the artifact appearing, in either direction.
test.describe('AllDayBand chip corners', () => {
  const CHIPS = ['plain', 'cont-left', 'cont-right', 'cont-both'];
  for (const [i, name] of CHIPS.entries()) {
    test(`a ${name} chip renders pixel for pixel`, async ({ page }) => {
      await page.goto(show('AllDayBand', 'corners'));
      await expect(page.locator('.chip').nth(i)).toHaveScreenshot(
        `allday-chip-${name}.png`,
        { maxDiffPixels: 0, maxDiffPixelRatio: 0, threshold: 0 },
      );
    });
  }
});

test.describe('Header', () => {
  test('disconnected state offers to connect', async ({ page }) => {
    await page.goto(show('Header', 'disconnected'));
    await expect(page.getByRole('button', { name: 'Connect Google Calendar' })).toBeVisible();
    await expect(page.locator('.synced')).toHaveCount(0);
    await expect(page.locator('header')).toHaveScreenshot('header-disconnected.png');
  });

  test('connected state shows the relative sync time', async ({ page }) => {
    // `relativeTime` reads the real wall clock, so the page clock is frozen to
    // the same instant the fixture's `last_sync_ms` is offset from — otherwise
    // the "N min ago" text (and any screenshot of it) drifts with the run date.
    await page.clock.setFixedTime(FIXED_NOW);
    await page.goto(show('Header', 'connected'));
    await expect(page.locator('.synced')).toHaveText('Synced 5 min ago');
    await expect(page.getByRole('button', { name: 'Sync now' })).toBeEnabled();
    await expect(page.locator('header')).toHaveScreenshot('header-connected.png');
  });

  test('the DEMO DATA badge appears when demo is true', async ({ page }) => {
    await page.goto(show('Header', 'demo'));
    await expect(page.locator('.demo')).toHaveText('DEMO DATA');
  });

  test('a connected demo account shows the badge but never offers Sync now', async ({ page }) => {
    // The real demo account is a seeded `accounts` row (connected), but was
    // never through OAuth — sync_now refuses it server-side, so the button
    // must not appear at all rather than invite a click that only errors.
    await page.clock.setFixedTime(FIXED_NOW);
    await page.goto(show('Header', 'connected-demo'));
    await expect(page.locator('.demo')).toHaveText('DEMO DATA');
    await expect(page.locator('.synced')).toHaveText('Synced 5 min ago');
    await expect(page.getByRole('button', { name: 'Sync now' })).toHaveCount(0);
    await expect(page.locator('header')).toHaveScreenshot('header-connected-demo.png');
  });

  test('busy disables the connect button while signing in', async ({ page }) => {
    await page.goto(show('Header', 'busy-disconnected'));
    const btn = page.getByRole('button', { name: 'Connecting…' });
    await expect(btn).toBeVisible();
    await expect(btn).toBeDisabled();
  });

  test('busy disables the sync button while syncing', async ({ page }) => {
    await page.clock.setFixedTime(FIXED_NOW);
    await page.goto(show('Header', 'busy-connected'));
    await expect(page.locator('.synced')).toHaveText('Syncing…');
    await expect(page.getByRole('button', { name: 'Sync now' })).toBeDisabled();
  });

  test('busy disables the add account button while syncing', async ({ page }) => {
    await page.clock.setFixedTime(FIXED_NOW);
    await page.goto(show('Header', 'busy-connected'));
    await expect(page.getByRole('button', { name: 'Add account' })).toBeDisabled();
  });

  test('a connected account can add another', async ({ page }) => {
    await page.goto(show('Header', 'connected'));
    await expect(page.getByRole('button', { name: 'Add account' })).toBeVisible();
  });

  test('a disconnected user is asked to connect, not to add', async ({ page }) => {
    await page.goto(show('Header', 'disconnected'));
    await expect(page.getByRole('button', { name: /Connect Google Calendar/ })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Add account' })).toHaveCount(0);
  });

  test('demo mode offers neither', async ({ page }) => {
    await page.goto(show('Header', 'demo'));
    await expect(page.getByRole('button', { name: 'Add account' })).toHaveCount(0);
    await expect(page.getByRole('button', { name: /Connect/ })).toHaveCount(0);
  });
});

test.describe('CalendarPopover', () => {
  const show = (f: string) => `/tests/harness/index.html?c=CalendarPopover&f=${f}`;

  test('opens and groups by account', async ({ page }) => {
    await page.goto(show('two-accounts'));
    await page.getByRole('button', { name: /Calendars/ }).click();
    await expect(page.locator('.acct')).toHaveCount(2);
  });

  // Task 7: a parent (`App`, right after sign-in) can drive the panel open
  // through the bindable `open` prop, without going through the trigger.
  test('a parent can open the picker', async ({ page }) => {
    await page.goto(show('open-on-mount'));
    await expect(page.locator('.panel')).toBeVisible();
  });

  test('it still starts closed by default', async ({ page }) => {
    await page.goto(show('two-accounts'));
    await expect(page.locator('.panel')).toHaveCount(0);
  });

  test('counts only calendars that are both synced and shown', async ({ page }) => {
    await page.goto(show('mixed'));
    // 3 calendars: one hidden, one removed, one visible.
    await expect(page.locator('.trigger .count')).toHaveText('1');
  });

  test('a removed calendar cannot be ticked', async ({ page }) => {
    await page.goto(show('mixed'));
    await page.getByRole('button', { name: /Calendars/ }).click();
    const off = page.locator('.row.off').first();
    await expect(off.locator('input[type=checkbox]')).toBeDisabled();
    await expect(off.locator('.sync')).toHaveText('Add');
  });

  test('clicking away closes it', async ({ page }) => {
    await page.goto(show('two-accounts'));
    await page.getByRole('button', { name: /Calendars/ }).click();
    await expect(page.locator('.panel')).toBeVisible();
    await page.locator('.scrim').click();
    await expect(page.locator('.panel')).toHaveCount(0);
  });

  test('Escape closes it', async ({ page }) => {
    await page.goto(show('two-accounts'));
    await page.getByRole('button', { name: /Calendars/ }).click();
    await expect(page.locator('.panel')).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(page.locator('.panel')).toHaveCount(0);
  });

  // Fix round 1, finding 1a: Tab once from the trigger and focus lands on
  // `.scrim` — a sibling of `.panel`, not a descendant of either element the
  // old per-element keydown handlers were attached to. Only a window-level
  // listener hears Escape from there.
  test('Escape closes it when focus is on the scrim', async ({ page }) => {
    await page.goto(show('two-accounts'));
    await page.getByRole('button', { name: /Calendars/ }).click();
    await page.locator('.scrim').focus();
    await page.keyboard.press('Escape');
    await expect(page.locator('.panel')).toHaveCount(0);
  });

  // Fix round 1, finding 1b: disabling a focused checkbox mid-toggle drops
  // focus to <body> (browser default, both engines) — nowhere a listener on
  // the trigger or the panel could ever hear from. Holds the call open so the
  // checkbox is still disabled, hence still stuck on <body>, when Escape
  // is pressed.
  test('Escape closes it once a toggle has moved focus to <body>', async ({ page }) => {
    await page.goto(show('single'));
    await page.evaluate(() => window.__harness.holdNextCalendarCall('set_calendar_selected'));
    await page.getByRole('button', { name: /Calendars/ }).click();

    const box = page.locator('input[type=checkbox]');
    await box.focus();
    await box.click(); // parked — the checkbox disables and focus falls to <body>
    await expect(box).toBeDisabled();

    await page.keyboard.press('Escape');
    await expect(page.locator('.panel')).toHaveCount(0);
  });

  // Resolution 4: the browser flips a checkbox's `checked` property on click,
  // before any handler runs. If `set_calendar_selected` then fails, the box
  // is left showing a state the store never actually reached until the
  // component explicitly snaps it back.
  test('a failed toggle snaps the checkbox back and reports the error', async ({ page }) => {
    await page.goto(show('single'));
    await page.evaluate(() =>
      window.__harness.failNextCalendarCall('set_calendar_selected', 'database is locked'),
    );
    await page.getByRole('button', { name: /Calendars/ }).click();

    const box = page.locator('input[type=checkbox]');
    await expect(box).toBeChecked(); // fixture calendar starts selected
    await box.click();

    // Fix round 1, finding 3: attributed to the calendar it's about — "Work"
    // is the `single` fixture's one calendar — so a note can't be misread as
    // belonging to some other row that settled around the same time.
    await expect(page.locator('.note.err')).toHaveText('Work · database is locked');
    // The click already flipped it once; a naive implementation stops here.
    await expect(box).toBeChecked();
  });

  // Fix round 1 (Task 7), finding 1: the `message = null` reset moved from
  // `toggle()` into an `$effect` keyed on `open`, so a parent-driven open
  // clears a stale note too. Nothing exercised that effect at all — a mutant
  // that drops the `open` read (runs the reset once, at mount, and never
  // again) left every existing test green. `message` is component state, not
  // DOM: it survives the panel unmounting on close, so a stale error from
  // before Escape must not still be showing after the panel reopens.
  test('reopening the panel clears a stale error message', async ({ page }) => {
    await page.goto(show('single'));
    await page.evaluate(() =>
      window.__harness.failNextCalendarCall('set_calendar_selected', 'database is locked'),
    );
    await page.getByRole('button', { name: /Calendars/ }).click();
    await page.locator('input[type=checkbox]').click();
    await expect(page.locator('.note.err')).toHaveText('Work · database is locked');

    await page.keyboard.press('Escape');
    await expect(page.locator('.panel')).toHaveCount(0);

    await page.getByRole('button', { name: /Calendars/ }).click();
    await expect(page.locator('.note')).toHaveCount(0);
  });

  // Fix round 1, finding 2: `busy` used to be a single id, so toggling a
  // second row while the first was still in flight pointed `busy` at the
  // second id and silently re-enabled the first — a real double-submit.
  // Holds the first row's call open and toggles the second immediately after,
  // so the two are genuinely concurrent rather than sequential.
  test('a row stays disabled until its own call resolves, even if another row toggles meanwhile', async ({ page }) => {
    await page.goto(show('mixed'));
    await page.evaluate(() => window.__harness.holdNextCalendarCall('set_calendar_selected'));
    await page.getByRole('button', { name: /Calendars/ }).click();

    // `mixed` has 3 calendars, one of them `sync_enabled: false` (`.row.off`,
    // permanently disabled); the other two are the rows this test toggles.
    const rows = page.locator('.row:not(.off) input[type=checkbox]');
    await rows.nth(0).click(); // consumes the hold — parked
    await rows.nth(1).click(); // no hold armed for it — resolves right away

    await expect(rows.nth(0)).toBeDisabled();
    await expect(rows.nth(1)).toBeEnabled();

    await page.evaluate(() => window.__harness.releaseCalendarCall('set_calendar_selected', undefined));
    await expect(rows.nth(0)).toBeEnabled();
  });

  // Resolution 1: `setCalendarSync` resolves with the number of events the
  // removal deleted specifically so the UI can report it — throwing that
  // count away would make the removal look like it did nothing. Fix round 1,
  // finding 3: also names the calendar, for the same reason as the failed-
  // toggle note above.
  test('removing a calendar reports how many events were deleted, naming the calendar', async ({ page }) => {
    await page.goto(show('single'));
    await page.getByRole('button', { name: /Calendars/ }).click();
    await page.getByRole('button', { name: 'Remove' }).click();
    await expect(page.locator('.note')).toHaveText(`Work · ${CALENDAR_SYNC_REMOVED} events deleted`);
  });
});

test.describe('EventPopover', () => {
  const show = (f: string) => `/tests/harness/index.html?c=EventPopover&f=${f}`;

  test('shows the guest list with each response', async ({ page }) => {
    await page.goto(show('standup'));
    await expect(page.locator('.guest')).toHaveCount(3);
    await expect(page.locator('.guest.accepted')).toHaveCount(1);
    await expect(page.locator('.guest.declined')).toHaveCount(1);
  });

  test('each guest carries a status glyph, and a different one per status', async ({ page }) => {
    // The whole point of the glyph is that "coming" and "hasn't replied" are
    // told apart at a glance. Asserting each mark's own character is what
    // catches a table that has gone uniform — a count of `.mark` would pass
    // even if every guest showed the same symbol.
    await page.goto(show('standup'));
    await expect(page.locator('.guest.accepted .mark')).toHaveText('✓');
    await expect(page.locator('.guest.declined .mark')).toHaveText('✕');
    await expect(page.locator('.guest.needsAction .mark')).toHaveText('?');
  });

  test('the status is announced, not only drawn', async ({ page }) => {
    // The ring is aria-hidden, so without the visually-hidden word a screen
    // reader would hear a name and nothing about whether they are coming.
    await page.goto(show('standup'));
    await expect(page.locator('.guest.accepted .sr')).toHaveText('accepted');
    await expect(page.locator('.guest.needsAction .sr')).toHaveText('no reply yet');
  });

  test('the panel claims to be modal and takes focus on open', async ({ page }) => {
    // The scrim already makes the grid behind unclickable. Without
    // `aria-modal` and the focus move, the tab order still begins wherever
    // the click left it — outside the panel, walking through a week of blocks
    // a mouse can no longer reach.
    //
    // Deliberately asserts where focus *starts*, not where Tab goes next:
    // WebKit only tabs to buttons and links when Full Keyboard Access is on,
    // so a Tab assertion here would be testing a browser preference. Focus
    // containment proper (wrapping Tab at the last control) is not
    // implemented — starting inside is what this covers.
    await page.goto(show('standup'));
    await expect(page.locator('.pop')).toHaveAttribute('aria-modal', 'true');
    await expect(page.locator('.pop')).toBeFocused();
  });

  test('a description containing markup is shown as text', async ({ page }) => {
    await page.goto(show('nasty-description'));
    await expect(page.locator('.desc')).toContainText('<script>alert(1)</script>');
    await expect(page.locator('.desc script')).toHaveCount(0);
  });

  test('a one-off event offers no scope choice', async ({ page }) => {
    await page.goto(show('standup'));
    await expect(page.locator('.rsvp')).toBeVisible();
    await expect(page.locator('.scope')).toHaveCount(0);
  });

  test('a recurring event asks which occurrences', async ({ page }) => {
    await page.goto(show('recurring'));
    await expect(page.locator('.scope')).toBeVisible();
    await expect(page.getByRole('radio', { name: /This one/ })).toBeChecked();
  });

  test('a read-only calendar offers no rsvp at all', async ({ page }) => {
    await page.goto(show('readonly'));
    await expect(page.locator('.guest')).toHaveCount(3);
    await expect(page.locator('.rsvp')).toHaveCount(0);
  });

  test('a failed response rolls the choice back and says why', async ({ page }) => {
    await page.goto(show('respond-fails'));
    await page.getByRole('button', { name: 'No' }).click();
    await expect(page.locator('.note.err')).toBeVisible();
    await expect(page.getByRole('button', { name: 'No' })).not.toHaveClass(/chosen/);
  });

  test('responding to a later occurrence sends that occurrence, not the series start', async ({ page }) => {
    // The trap named in the Interfaces block: `detail.start_ms` is the series
    // DTSTART for a master row, and passing it silently patches occurrence #0
    // for everyone. Assert the fourth argument is the clicked block's own start.
    await page.goto(show('recurring-fourth-occurrence'));
    await page.getByRole('button', { name: 'No' }).click();
    const call = await page.evaluate(() => (window as any).__lastRespondCall);
    expect(call.occurrenceStartMs).toBe(1786600800000); // Thu 13 Aug, the clicked block
    expect(call.occurrenceStartMs).not.toBe(1786341600000); // Mon 10 Aug, the series start
    // The scope radio defaults to "this" (asserted by the recurring-event
    // spec above), and nothing here touched it — the call must say so too.
    expect(call.scope).toBe('this');
  });

  test('choosing "All of them" sends that scope, not the default', async ({ page }) => {
    await page.goto(show('recurring-fourth-occurrence'));
    await page.getByRole('radio', { name: 'All of them' }).check();
    await page.getByRole('button', { name: 'No' }).click();
    const call = await page.evaluate(() => (window as any).__lastRespondCall);
    expect(call.scope).toBe('all');
  });

  test('a successful response shows immediately, without waiting for a sync', async ({ page }) => {
    // The backend deliberately returns the master's unchanged detail after a
    // "this one" RSVP, so nothing moves on screen unless the UI reflects the
    // choice itself. Five minutes of a dead button reads as a failure.
    await page.goto(show('recurring-fourth-occurrence'));
    await page.getByRole('button', { name: 'No' }).click();
    await expect(page.getByRole('button', { name: 'No' })).toHaveClass(/chosen/);
  });

  test('a successful non-recurring response also updates the guest list, not just the buttons', async ({ page }) => {
    // Unlike the bare-master "this one" case above, the backend really does
    // write back here (every non-recurring event, and `scope: 'all'`) — the
    // guest list's own "you" row must catch up too, or the buttons would say
    // "No" while the row right below them still reads needsAction.
    await page.goto(show('writes-back'));
    await page.getByRole('button', { name: 'No' }).click();
    await expect(page.locator('.guest')).toHaveClass(/declined/);
  });

  test('escape closes it even when focus has fallen to the body', async ({ page }) => {
    // Plan 1c shipped this bug once: a keydown handler on the panel misses
    // Escape entirely once a disabled control drops focus to <body>, and a
    // test that only presses Escape with the trigger focused cannot see it.
    await page.goto(show('standup'));
    await expect(page.locator('.pop')).toBeVisible();
    await page.evaluate(() => (document.activeElement as HTMLElement)?.blur());
    expect(await page.evaluate(() => document.activeElement?.tagName)).toBe('BODY');
    await page.keyboard.press('Escape');
    await expect(page.locator('.pop')).toHaveCount(0);
  });

  test('clicking a guest list does not close it', async ({ page }) => {
    // The scrim must sit behind the panel, not over it.
    await page.goto(show('standup'));
    await page.locator('.guest').first().click();
    await expect(page.locator('.pop')).toBeVisible();
  });
});

test.describe('MonthGrid', () => {
  const show = (f: string) => `/tests/harness/index.html?c=MonthGrid&f=${f}`;

  test('renders six rows of seven, with out-of-month days dimmed', async ({ page }) => {
    await page.goto(show('august'));
    await expect(page.locator('.mrow')).toHaveCount(6);
    await expect(page.locator('.mcell')).toHaveCount(42);
    await expect(page.locator('.mcell.out')).toHaveCount(11); // 5 leading + 6 trailing
  });

  test('a multi-day event is one bar, not one chip per day', async ({ page }) => {
    await page.goto(show('august'));
    await expect(page.locator('.bar', { hasText: 'Berlin trip' })).toHaveCount(1);
  });

  test('two co-existing bars in one row each keep their own title', async ({ page }) => {
    // `august` never packs more than one bar per row, so `idx` and `lane`
    // never diverge there — a `bar_events[lane.idx]` / `bar_events[lane.lane]`
    // mix-up would still pass every other spec in this file.
    await page.goto(show('two-bars'));
    const bars = page.locator('.bar');
    await expect(bars).toHaveCount(2);
    await expect(bars.nth(0)).toContainText('Berlin trip');
    await expect(bars.nth(1)).toContainText('Team offsite');
  });

  test('a timed event shows a dot and a title, and no time', async ({ page }) => {
    // Spec §2: a time prefix costs about a third of a narrow cell.
    await page.goto(show('august'));
    const line = page.locator('.mcell .timed').first();
    await expect(line).toContainText('Standup');
    await expect(line).not.toContainText(':');
  });

  test('+N more asks the parent for that day', async ({ page }) => {
    await page.goto(show('busy-day'));
    await page.locator('.more').first().click();
    const picked = await page.evaluate(() => (window as any).__lastDayPick);
    expect(picked).toBe(1786341600000); // Mon 10 Aug
  });

  test('clicking the day number asks the parent for that day too', async ({ page }) => {
    await page.goto(show('august'));
    await page.locator('.mcell .num').nth(14).click();
    expect(await page.evaluate(() => (window as any).__lastDayPick)).toBeTruthy();
  });

  test('clicking an event opens the popover, not the day', async ({ page }) => {
    await page.goto(show('august'));
    await page.locator('.mcell .timed').first().click();
    expect(await page.evaluate(() => (window as any).__lastOpen)).toBeTruthy();
    expect(await page.evaluate(() => (window as any).__lastDayPick)).toBeFalsy();
  });

  test('a timed line keeps a real, readable height', async ({ page }) => {
    // `MAX_BAR_LANES`'s own comment explains why: unlike `BigYearRibbon`,
    // `.bars` here is deliberately content-sized rather than reserving
    // `MAX_BAR_LANES` fixed tracks with `grid-template-rows` — a month row
    // has only ~95px to divide, and a reserved bar strip leaves too little
    // for the cell below, measured to squeeze every timed line down to
    // 0.05px. Healthy is ~10px; 4px sits with margin on both sides of that
    // gap without pinning to the exact pixel value.
    await page.goto(show('busy-day'));
    const line = page.locator('.mcell .timed').first();
    expect((await line.boundingBox())!.height).toBeGreaterThan(4);
  });
});

test.describe('YearGrid', () => {
  const show = (f: string) => `/tests/harness/index.html?c=YearGrid&f=${f}`;

  test('renders twelve months', async ({ page }) => {
    await page.goto(show('y2026'));
    await expect(page.locator('.ymonth')).toHaveCount(12);
  });

  test('a day with an all-day event gets a dot', async ({ page }) => {
    await page.goto(show('y2026'));
    await expect(page.locator('.yday.dotted')).toHaveCount(1);
  });

  test('today is a filled disc', async ({ page }) => {
    // `YearGrid` reads the real wall clock; `y2026` is a fixed calendar year,
    // so the clock must be frozen to an instant inside it — same pattern as
    // `FIXED_NOW` for the Header specs below — or this becomes a permanent
    // failure the moment the real date rolls past 2026.
    await page.clock.setFixedTime(YEAR_2026_NOW);
    await page.goto(show('y2026'));
    await expect(page.locator('.yday.today')).toHaveCount(1);
  });

  test('unsynced days are distinct from empty ones', async ({ page }) => {
    // §6: an empty January must not read as a free January.
    await page.goto(show('y2026'));
    const unsynced = page.locator('.yday.unsynced').first();
    await expect(unsynced).toBeVisible();
    await expect(unsynced).not.toHaveClass(/dotted/);
  });

  test('clicking a date asks the parent for that day', async ({ page }) => {
    await page.goto(show('y2026'));
    await page.locator('.yday').nth(200).click();
    expect(await page.evaluate(() => (window as any).__lastDayPick)).toBeTruthy();
  });
});

test.describe('BigYearRibbon', () => {
  const show = (f: string) => `/tests/harness/index.html?c=BigYearRibbon&f=${f}`;

  test('renders fourteen rows of twenty-eight', async ({ page }) => {
    await page.goto(show('y2026'));
    await expect(page.locator('.rrow')).toHaveCount(14);
    await expect(page.locator('.rrow').first().locator('.rday')).toHaveCount(28);
  });

  test('weekend shading forms straight vertical stripes', async ({ page }) => {
    // The 28-day row exists for this. Assert the column indices, not a
    // screenshot: this is the property, and a screenshot would also pass
    // for a subtly different one.
    await page.goto(show('y2026'));
    for (const r of [0, 7, 13]) {
      const cols: number[] = [];
      const days = page.locator('.rrow').nth(r).locator('.rday');
      for (let i = 0; i < 28; i++) {
        if ((await days.nth(i).getAttribute('class'))?.includes('wknd')) cols.push(i);
      }
      expect(cols).toEqual([5, 6, 12, 13, 19, 20, 26, 27]);
    }
  });

  test('days outside the year are dimmed, not blank', async ({ page }) => {
    await page.goto(show('y2026'));
    const out = page.locator('.rday.out').first();
    await expect(out).toBeVisible();
    await expect(out).not.toBeEmpty();
  });

  test('a span crossing a row shows a continuation marker on both halves', async ({ page }) => {
    await page.goto(show('crossing'));
    await expect(page.locator('.pill.cont')).toHaveCount(2);
  });

  test('the legend names each calendar that has a pill', async ({ page }) => {
    await page.goto(show('y2026'));
    await expect(page.locator('.legend .item')).toHaveCount(2);
  });

  test('two calendars sharing a name still render the whole ribbon', async ({ page }) => {
    // Two accounts subscribed to the same public calendar report the same
    // `summary`, which `get_big_year` copies verbatim into `name`. Keying the
    // legend by `name` makes Svelte 5 throw `each_key_duplicate`, and that is
    // not a broken legend — the component never mounts, so the rows go with
    // it. The rows are asserted first for exactly that reason: the legend
    // count alone would not say which failure mode this is guarding.
    await page.goto(show('same-name-legend'));
    await expect(page.locator('.rrow')).toHaveCount(14);
    await expect(page.locator('.pill')).toHaveCount(2);
    await expect(page.locator('.legend .item')).toHaveCount(2);
  });

  test('clicking a pill opens the popover', async ({ page }) => {
    await page.goto(show('y2026'));
    await page.locator('.pill').first().click();
    expect(await page.evaluate(() => (window as any).__lastOpen)).toBeTruthy();
  });

  test('a fully packed row keeps its day strip, and its weekend stripes with it', async ({ page }) => {
    // Measured with the old (unreserved-lanes, unprotected-min-height) layout
    // reinstated: `.rdays` itself collapsed to 0 height, but each `.rday` kept
    // its own 15px content height and painted at the y-coordinate `.rdays`
    // would have started at — inside the row *below*, since that row's own
    // strip spans only ~37px. The stripe was never missing; it was painted in
    // the wrong place, overlapping the next row's days. With `.pills`
    // content-sized, row 0 here (three lanes plus a "+N more") is what drove
    // `.rdays` into that squeeze, while every quieter row was fine — so the
    // busiest row, the one that most needs reading, was the one that lost the
    // property. Only the `.rdays` assertion below binds: `.rday.wknd`'s own
    // height stays 15px whether or not the bug is present (see the boxes
    // above), so a bounding-box assertion on it can never catch this.
    await page.goto(show('three-lanes'));
    const packed = page.locator('.rrow').first();

    // `RESERVED_PILL_LANES` (2) trims the CSS budget below `PILL_LANE_CAP`
    // (3, `pack_lanes`'s own cap) — reservation and cap are deliberately not
    // the same number any more, so this is what proves the split didn't
    // quietly become a cap of its own: all three of this row's genuinely
    // overlapping spans still render, the row just grows past its reserved
    // budget to fit the one it doesn't have a track for.
    await expect(packed.locator('.pill')).toHaveCount(3);

    expect((await packed.locator('.rdays').boundingBox())!.height).toBeGreaterThan(0);

    // And the day strip is the same height in a packed row as in an empty
    // one: `.rdays` sits at its own protected minimum regardless of how tall
    // `.pills` grows beside it — true even here, where this row's third lane
    // and overflow both exceed `RESERVED_PILL_LANES`'s budget and grow the
    // row past it (see that constant's own comment). Without *any* lane
    // reservation, a quiet row's `.pills` collapses toward nothing instead,
    // and its `.rdays` inflates to absorb the slack — so the two diverge.
    const quiet = page.locator('.rrow').nth(2);
    const packedDays = (await packed.locator('.rdays').boundingBox())!.height;
    const quietDays = (await quiet.locator('.rdays').boundingBox())!.height;
    expect(Math.abs(packedDays - quietDays)).toBeLessThan(1);
  });

  test('all fourteen rows fit on one screen with no scroll', async ({ page }) => {
    // The design doc's own promise for this view (spec §4): "Big Year — one
    // screen, the whole year." `RESERVED_PILL_LANES`'s comment explains the
    // budget this depends on: 14 rows at a uniform ~35px fit inside the
    // ~530px container the suite's default 1280x720 viewport gives `.rows`
    // (`devices['Desktop Chrome']`/`['Desktop Safari']` in
    // playwright.config.ts — no `setViewportSize` here, same as every other
    // spec in this file). Pinned so that budget can't drift back out of
    // reach without a spec noticing, the way the original 3-lane reservation
    // did.
    await page.goto(show('y2026'));
    await expect(page.locator('.rrow')).toHaveCount(14);
    const overflow = await page.locator('.rows').evaluate((el) => el.scrollHeight - el.clientHeight);
    expect(overflow).toBeLessThanOrEqual(0);
  });

  test('days outside the synced window are hatched, not left blank', async ({ page }) => {
    // §6: the window opens 180 days back, so a ribbon anchored the previous
    // December always begins outside it. Nothing else distinguishes those days
    // from an in-window day with nothing on it, so without this an unsynced
    // stretch reads as "free".
    await page.goto(show('unsynced'));
    await expect(page.locator('.rrow').nth(0).locator('.rday.unsynced')).toHaveCount(28);
    await expect(page.locator('.rrow').nth(1).locator('.rday.unsynced')).toHaveCount(28);
    await expect(page.locator('.rrow').nth(2).locator('.rday.unsynced')).toHaveCount(0);
    // The hatch is a real painted background, not just a class name.
    const hatched = page.locator('.rday.unsynced').first();
    expect(await hatched.evaluate((el) => getComputedStyle(el).backgroundImage))
      .toContain('repeating-linear-gradient');
  });

  test('two co-existing pills in one row each keep their own title', async ({ page }) => {
    // `y2026` and `crossing` never pack more than one pill per row, so `idx`
    // and `lane` never diverge there — a `pill_events[lane.idx]` /
    // `pill_events[lane.lane]` mix-up would still pass every other spec in
    // this file. Same guard as MonthGrid's `two-bars` spec.
    await page.goto(show('two-pills'));
    const pills = page.locator('.pill');
    await expect(pills).toHaveCount(2);
    await expect(pills.nth(0)).toContainText('Berlin trip');
    await expect(pills.nth(1)).toContainText('Team offsite');
  });
});
