import { test, expect, type Page } from '@playwright/test';
import {
  FIXED_NOW, FORM_FALLBACK_ID, FORM_NOW, FORM_UNWRITABLE_ID, FORM_UNWRITABLE_NAMES,
  MON, MONTH_2026_NOW, POPOVER_DETAILS, POPOVER_REFRESHED_DETAIL,
  TRIP_END_DATE, TRIP_FIRST_DAY, TRIP_LAST_DAY, popoverWeekWithResponse, YEAR_2026_NOW,
} from './fixtures';
import { CALENDAR_SYNC_REMOVED } from './harness/tauri';

const show = (c: string, f: string) => `/tests/harness/index.html?c=${c}&f=${f}`;

test.describe('WeekGrid', () => {
  test('renders an empty week', async ({ page }) => {
    await page.goto(show('WeekGrid', 'empty'));
    await expect(page.locator('.col')).toHaveCount(7);
    await expect(page).toHaveScreenshot('weekgrid-empty.png');
  });

  // Task 10. Exactly on the 10:00 line, which is where somebody aims to make a
  // 10:00 meeting — and the one place the empty-space target does not receive
  // the click unless the hour rules are made transparent to the pointer. They
  // are positioned after it in the column, so a point within half a pixel of a
  // line returns `.rule` from `elementFromPoint` without
  // `pointer-events: none`; measured in both engines. A 1px dead band every
  // two hours is not something a user would ever report as a bug, only as the
  // app "sometimes not doing anything".
  test('clicking exactly on an hour line still asks for a new event there', async ({ page }) => {
    await page.goto(show('WeekGrid', 'empty'));
    const col = page.locator('.col').first();
    const box = (await col.boundingBox())!;
    await col.click({ position: { x: box.width / 2, y: box.height * (10 / 24) } });
    expect(await page.evaluate(() => (window as any).__lastCreate)).toMatchObject({
      startMs: MON + 10 * 3_600_000,
    });
  });

  // The same guard for the current-time line, which is the worse of the two:
  // it is 1.5px plus a 7px dot, and it crawls down today's column all day, so
  // the dead band it makes is both bigger and moving.
  //
  // I first left this unspec'd on the grounds that reaching `.now` needed a
  // fixture whose week moves with the calendar. That was wrong, and the fix is
  // the pattern this suite already uses for `YearGrid`'s today-highlight: the
  // fixture stays fixed in the past and the *clock* moves to it. Frozen at
  // 10:20 on `MON` itself, the first column becomes today, `.now` renders at
  // 10:20 — and the click at 10:00 lands on the hour line while the dot sits
  // 20 minutes below, so this covers `.rule` and `.now` at once without
  // needing to know exactly where the dot fell.
  test('the current-time line does not swallow a click either', async ({ page }) => {
    await page.clock.setFixedTime(MON + 10 * 3_600_000 + 20 * 60_000);
    await page.goto(show('WeekGrid', 'empty'));
    await expect(page.locator('.col.today .now')).toHaveCount(1);

    const col = page.locator('.col').first();
    const box = (await col.boundingBox())!;
    // Straight through the dot: it is drawn by `.now::before` at the line's
    // own left edge, so this is the pixel most likely to be intercepted.
    await col.click({ position: { x: 3, y: box.height * (10 + 20 / 60) / 24 } });
    expect(await page.evaluate(() => (window as any).__lastCreate)).toMatchObject({
      startMs: MON + 10 * 3_600_000,
    });
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

  // Task 10, and the same property as the spec above through a different code
  // path: Edit and Delete hand the caller an `Occurrence`, and its `startMs`
  // must be the clicked block's, never `detail.start_ms`. Both controls in one
  // spec, because they are the same relay called twice — and the popover has
  // to be gone by the time either lands, or the form would open behind a scrim
  // that is still there.
  test('edit and delete hand up the clicked block, and close the popover', async ({ page }) => {
    const seriesStart = POPOVER_DETAILS[42].start_ms;
    const blockStart = seriesStart + 3 * 24 * 3_600_000;

    await page.goto(show('popover'));
    await page.getByRole('button', { name: 'Standup' }).click();
    await expect(page.locator('.pop')).toBeVisible();
    await page.getByRole('button', { name: 'Edit' }).click();
    await expect(page.locator('.pop')).toHaveCount(0);
    const edit = await page.evaluate(() => (window as any).__lastEdit);
    expect(edit.occurrence.startMs).toBe(blockStart);
    expect(edit.occurrence.startMs).not.toBe(seriesStart);
    expect(edit.occurrence.endMs).toBe(blockStart + 30 * 60_000);

    await page.getByRole('button', { name: 'Standup' }).click();
    await expect(page.locator('.pop')).toBeVisible();
    await page.getByRole('button', { name: 'Delete' }).click();
    await expect(page.locator('.pop')).toHaveCount(0);
    const del = await page.evaluate(() => (window as any).__lastDelete);
    expect(del.occurrence.startMs).toBe(blockStart);
    expect(del.occurrence.startMs).not.toBe(seriesStart);
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

  // Task 10. A pair, deliberately: `detail()`'s own default is
  // `can_edit: false`, so the "shown" half fails until its fixture opts in
  // explicitly, and the "hidden" half is the one that could pass vacuously —
  // which is the safe way round, since an absent control ships nothing to
  // somebody who may not use it. Together they discriminate both ways.
  test('an event the user can write to offers Edit and Delete', async ({ page }) => {
    await page.goto(show('editable'));
    await expect(page.getByRole('button', { name: 'Edit' })).toHaveCount(1);
    await expect(page.getByRole('button', { name: 'Delete' })).toHaveCount(1);
  });

  test('an event the user cannot write to offers neither', async ({ page }) => {
    // Offering either on a calendar this account only reads would produce a
    // Save — or a Delete confirmation with no undo behind it — that
    // `update_impl`'s own writability check could only refuse, after the user
    // had already decided to go through with it.
    await page.goto(show('standup'));
    await expect(page.locator('.pop')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Edit' })).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Delete' })).toHaveCount(0);
  });

  test('clicking a guest list does not close it', async ({ page }) => {
    // The scrim must sit behind the panel, not over it.
    await page.goto(show('standup'));
    await page.locator('.guest').first().click();
    await expect(page.locator('.pop')).toBeVisible();
  });

  // Task 6's sweep. The `when` line rendered `detail.start_ms` through
  // `toLocaleDateString`/`toLocaleTimeString` in the browser's zone, and no
  // spec asserted on it at all — five plans, and the two fixtures below are
  // the first that could ever have caught it, because every other one here
  // has `occurrenceStartMs === detail.start_ms`.
  //
  // For a series `detail.start_ms` is the **master's** DTSTART. The block on
  // the grid beside this panel is drawn from the occurrence's own `start_ms`,
  // so the two disagreeing puts a popover on screen contradicting the thing it
  // was opened from.
  test('a later occurrence shows its own day and clock, not the master’s', async ({ page }) => {
    await page.goto(show('recurring-across-a-fall-back'));

    // The fixture's own premise, asserted rather than described: seven days
    // and **one hour** apart, because Sofia's clocks go back between them. A
    // fixture that stopped straddling would still separate the two dates but
    // could no longer separate the two clocks, and half of this spec would
    // pass vacuously.
    const gapHours = await page.evaluate(() => {
      const f = (window as any).__fixtureProps;
      return (f.occurrenceStartMs - f.detail.start_ms) / 3_600_000;
    });
    expect(gapHours).toBe(169);

    const when = page.locator('.when');
    await expect(when).toContainText('Mon, Oct 26');
    await expect(when).toContainText('07:00');
    await expect(when).toContainText('07:30');
    // The master's own day and clock, which is what this used to show.
    await expect(when).not.toContainText('Oct 19');
    await expect(when).not.toContainText('06:00');
  });
});

// The all-day arm of the same line, in a browser **west** of UTC.
//
// Three separate ways to get this wrong, and the zone above is what makes the
// third visible: an all-day day rebuilt from a `yyyy-mm-dd` through `Date.UTC`
// and then formatted without `timeZone: 'UTC'` is put straight back through the
// browser's zone, which east of the reader is the previous day. From the
// project's default UTC browser that mistake is invisible.
test.describe('EventPopover on an all-day series east of the browser', () => {
  test.use({ timezoneId: 'America/New_York' });

  test('shows the clicked day, in the calendar’s zone, not a reading of an instant', async ({ page }) => {
    await page.goto('/tests/harness/index.html?c=EventPopover&f=all-day-series-east-of-the-browser');

    // Both premises, in the page so they are read in the browser's own zone.
    // Unless this browser reads the stored instants as *different* days from
    // the ones the calendar keeps them on, every assertion below is satisfied
    // by the fixture rather than by the component.
    const premise = await page.evaluate(() => {
      const f = (window as any).__fixtureProps;
      const day = (ms: number) =>
        new Date(ms).toLocaleDateString([], { weekday: 'short', month: 'short', day: 'numeric' });
      return {
        browserReadsRow: day(f.detail.start_ms),
        browserReadsOccurrence: day(f.occurrenceStartMs),
        rowDate: f.detail.start_date,
        shiftDays: (f.occurrenceStartMs - f.detail.start_ms) / 86_400_000,
      };
    });
    // The calendar keeps the master on the 10th; this browser reads that same
    // instant as the 9th, and the clicked chip's as the 12th. The right answer
    // — the 13th — is a date neither reading produces.
    expect(premise.rowDate).toBe('2026-08-10');
    expect(premise.browserReadsRow).toBe('Sun, Aug 9');
    expect(premise.browserReadsOccurrence).toBe('Wed, Aug 12');
    expect(premise.shiftDays).toBe(3);

    await expect(page.locator('.when')).toHaveText('Thu, Aug 13');
  });

  test('an all-day event shows no clock at all', async ({ page }) => {
    // The `{#if !detail.is_all_day}` guard, which is the reason the times may
    // be read off instants on the other arm without this one having to care.
    await page.goto('/tests/harness/index.html?c=EventPopover&f=all-day-series-east-of-the-browser');
    await expect(page.locator('.when')).not.toContainText(':');
  });
});

test.describe('MonthGrid', () => {
  const show = (f: string) => `/tests/harness/index.html?c=MonthGrid&f=${f}`;

  // `MonthGrid` computes `todayStart` from `new Date()` while every fixture
  // here is a fixed August 2026, so the clock is frozen ahead of *every*
  // navigation in this block, not only the today-highlight spec below. Two
  // separate reasons, and only the first is about the new spec: an unfrozen
  // clock leaves the highlight untestable, and it also leaves the rest of the
  // block quietly reading wall-clock time in a suite that otherwise controls
  // it. Same mechanism as `App`'s own `beforeEach` and `YearGrid`'s
  // `YEAR_2026_NOW`; it has to run before `page.goto`, since the component
  // reads the clock once, at mount.
  test.beforeEach(async ({ page }) => {
    await page.clock.setFixedTime(MONTH_2026_NOW);
  });

  test('renders six rows of seven, with out-of-month days dimmed', async ({ page }) => {
    await page.goto(show('august'));
    await expect(page.locator('.mrow')).toHaveCount(6);
    await expect(page.locator('.mcell')).toHaveCount(42);
    await expect(page.locator('.mcell.out')).toHaveCount(11); // 5 leading + 6 trailing
  });

  test('exactly one cell is today, and it is the day the clock is on', async ({ page }) => {
    // Both halves of the claim, because either alone is satisfied by a real
    // defect. `toHaveCount(1)` on `.mcell.today` says nothing about *which*
    // cell; asserting the 10th carries the class passes just as happily while
    // the 9th carries it too — turning the `===` into a `<=` highlights every
    // day up to today and would still find the 10th among them. So the whole
    // 42-cell vector is compared at once, the way the ribbon's weekend-stripe
    // spec compares all 28 of its columns rather than sampling one.
    await page.goto(show('august'));
    const cells = page.locator('.mcell');
    // Retried, so it also establishes that the component actually mounted —
    // and it is what makes the fixed loop bound below cover every cell there
    // is, rather than the first 42 of however many exist.
    await expect(cells).toHaveCount(42);

    const flagged: number[] = [];
    for (let i = 0; i < 42; i++) {
      if ((await cells.nth(i).getAttribute('class'))?.includes('today')) flagged.push(i);
    }
    // Row 2, column 0 — Mon 10 Aug 2026, the day `MONTH_2026_NOW` is 14:00 on.
    expect(flagged).toEqual([14]);
    // Not implied by the vector above, and not a restatement of it: that one
    // says which *cell* carries the class, this one says that cell is the one
    // showing the 10th. They read different things — a class attribute and a
    // rendered date — so a day number computed one day out leaves the vector
    // untouched and fails here alone.
    await expect(cells.nth(14).locator('.num')).toHaveText('10');
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
    expect(picked).toBe(1786320000000); // Mon 10 Aug 2026 00:00 UTC, the busy cell's own start
  });

  test('clicking the day number asks the parent for that day too', async ({ page }) => {
    await page.goto(show('august'));
    await page.locator('.mcell .num').nth(14).click();
    expect(await page.evaluate(() => (window as any).__lastDayPick)).toBeTruthy();
  });

  // Task 10. Both halves in one spec, because the risk is precisely that they
  // become the same click: the empty-space target covers the whole cell, and
  // the day number keeps its own click only by sitting above it. Invert the
  // `z-index` pair in `MonthGrid`'s styles and the second half fails — the
  // target swallows the number.
  test('empty cell space asks for a new event on that day, and the day number still does not', async ({ page }) => {
    // The grid's first cell, Mon 27 Jul, which carries nothing but its own
    // number — so a click on the middle of it is genuinely empty space and
    // nothing else could have answered.
    await page.goto(show('august'));
    const cell = page.locator('.mcell').first();

    await cell.locator('.newhere').click();
    expect(await page.evaluate(() => (window as any).__lastCreate)).toMatchObject({
      startMs: Date.UTC(2026, 6, 27),
    });
    expect(await page.evaluate(() => (window as any).__lastDayPick)).toBeFalsy();

    await cell.locator('.num').click();
    expect(await page.evaluate(() => (window as any).__lastDayPick)).toBe(Date.UTC(2026, 6, 27));
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

  // Lifted out of the today-highlight spec below, which used to be the only
  // one here that froze the clock — the other four still read the run date
  // through `YearGrid`'s own `todayStart`. Nothing they assert can observe it
  // today (the `today` class is independent of `dotted` and `unsynced`, and
  // this block takes no screenshots), so this changes no result; it removes a
  // wall-clock read from four more spec paths, which is the same thing being
  // done for `MonthGrid` above.
  test.beforeEach(async ({ page }) => {
    await page.clock.setFixedTime(YEAR_2026_NOW);
  });

  test('renders twelve months', async ({ page }) => {
    await page.goto(show('y2026'));
    await expect(page.locator('.ymonth')).toHaveCount(12);
  });

  test('a day with an all-day event gets a dot', async ({ page }) => {
    await page.goto(show('y2026'));
    await expect(page.locator('.yday.dotted')).toHaveCount(1);
  });

  test('today is a filled disc, on the right day and no other', async ({ page }) => {
    // `YearGrid` reads the real wall clock; `y2026` is a fixed calendar year,
    // so the clock must be frozen to an instant inside it — same pattern as
    // `FIXED_NOW` for the Header specs above — or this becomes a permanent
    // failure the moment the real date rolls past 2026. That freeze is the
    // `beforeEach` at the top of this block.
    //
    // This was `expect('.yday.today').toHaveCount(1)`, which pinned *how many*
    // discs and not *which day*: shift the highlight a day and the count is
    // still exactly 1, so the spec passed while the grid marked the wrong
    // date. The month-by-month vector below pins both at once — the same
    // treatment `MonthGrid`'s today spec gets, in the shape this grid has.
    //
    // Read per month rather than as one flat 365-long list because that is the
    // claim worth making: *no other month* has a disc, and June's is the 10th.
    // A flat day-of-year index would say the same thing in a number nobody can
    // check by eye.
    await page.goto(show('y2026'));
    const months = page.locator('.ymonth');
    // Retried, so it also establishes the component mounted — and it is what
    // makes the fixed loop bound below cover every month there is. `YearGrid`
    // takes its payload as a prop and renders in one pass, so once the twelve
    // months are here, so is every `.yday` inside them.
    await expect(months).toHaveCount(12);

    const flagged: string[][] = [];
    for (let i = 0; i < 12; i++) {
      flagged.push(await months.nth(i).locator('.yday.today').allTextContents());
    }
    // `YEAR_2026_NOW` is Wed 10 Jun 2026, so June — index 5 — and nothing else.
    expect(flagged).toEqual([[], [], [], [], [], ['10'], [], [], [], [], [], []]);
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

  // ---- a title appears once, not on every row ------------------------------

  /** `crossingBigYear`'s span, which is the only text either of its pills
   *  could carry. Read from one place so "the head has it" and "the tail does
   *  not" cannot drift apart. */
  const CROSSING_TITLE = 'Sun-Tue trip';

  test('a title is printed once, on the segment that starts the run', async ({ page }) => {
    // `crossing` is one event across a row boundary: `rows[0]` holds the head
    // (columns 25-27, `cont_right`) and `rows[1]` the tail (columns 0-1,
    // `cont_left`), in that DOM order. A three-row conference used to print its
    // name three times.
    await page.goto(show('crossing'));
    const pills = page.locator('.pill');
    await expect(pills).toHaveCount(2);
    await expect(pills.nth(0)).toHaveText(CROSSING_TITLE);
    // Bare, which also covers the `‹` that used to be the tail's first
    // characters — an empty segment cannot be carrying a chevron.
    await expect(pills.nth(1)).toHaveText('');
  });

  test('a bare continuation is still a pill: it spans its days and it opens', async ({ page }) => {
    // The other half of "shows no text": everything else about the segment is
    // unchanged. Without this, deleting the tail element outright would satisfy
    // the spec above.
    await page.goto(show('crossing'));
    const tail = page.locator('.pill.cl');

    // Two of the row's twenty-eight columns, because the span runs into day 2 —
    // measured against a day in the same row rather than written as a pixel
    // count, so it stays true at any width. The fill is what carries the run
    // now that the text is gone, so a tail collapsed to nothing would be the
    // whole feature lost.
    const tailBox = (await tail.boundingBox())!;
    const dayBox = (await page.locator('.rrow').nth(1).locator('.rday').first().boundingBox())!;
    expect(tailBox.width).toBeGreaterThan(dayBox.width);
    expect(tailBox.width).toBeLessThan(3 * dayBox.width);

    await tail.click();
    expect(await page.evaluate(() => (window as any).__lastOpen?.event?.title))
      .toBe(CROSSING_TITLE);
  });

  test('a continuation segment still has an accessible name', async ({ page }) => {
    // The regression the change above would otherwise have shipped: the tail's
    // only content *was* the title, so it became a `<button>` with nothing in
    // it, and a control with no name is unreachable by name to anything driving
    // the app through the accessibility tree.
    //
    // Both assertions are here on purpose and only the first one bites if the
    // `aria-label` is removed. Measured, by deleting the label and running the
    // `getByRole` line ahead of the other: it still finds both buttons, in
    // WebKit and in Chromium — `title` is also on the element and both engines
    // fall back to it for the accessible name. So the second line cannot be
    // this test's witness, which is exactly the reason the attribute is
    // asserted directly rather than through the name.
    //
    // It is still worth having: it is the one that says the name genuinely
    // resolves in each engine, rather than that an attribute is spelled right.
    // The point of the label is that the name stops depending on a fallback the
    // two engines are free to disagree about, and only one of these two lines
    // can see that the fallback is gone.
    await page.goto(show('crossing'));
    await expect(page.locator('.pill.cl')).toHaveAttribute('aria-label', CROSSING_TITLE);
    await expect(page.getByRole('button', { name: CROSSING_TITLE, exact: true })).toHaveCount(2);
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

  // ---- solid pills: choosing an ink against the fill, not against the theme --
  //
  // `foregroundFor` itself is tabled in `ink.spec.ts`, in Node and without a
  // browser context. What is left here is the part that is genuinely about CSS:
  // that the ink it picks reaches the pill, that the fill really is the
  // calendar's colour at full strength, and that the two edge cases the solid
  // fill created — an unparseable `--cal`, and a continuation marker that used
  // to be drawn in the fill's own colour — still leave something readable.

  /** Theme variables as the engine computes them.
   *
   *  `rgba(0,0,0,.88)` and `rgba(0, 0, 0, 0.88)` are one colour and two
   *  strings, so a spec comparing a pill's computed `color` against
   *  `theme.ts`'s literal would be failing on serialisation rather than on the
   *  property. Putting both sides through the same probe normalises that.
   *
   *  Resolved rather than copied so these tests say "the ink the theme
   *  publishes for a light fill" and not "rgba(0, 0, 0, 0.88)". A copy of those
   *  literals here would be a second place that knows them, which is the thing
   *  `theme.ts` exists to prevent. */
  const resolveInks = (page: Page, vars: string[]) => page.evaluate((vs: string[]) => {
    const probe = document.createElement('span');
    document.body.appendChild(probe);
    const out = vs.map((v) => {
      probe.style.color = v;
      return getComputedStyle(probe).color;
    });
    probe.remove();
    return out;
  }, vars);

  test('a pale fill and a dark one on the same row take different inks', async ({ page }) => {
    // The rendered half of `ink.spec.ts`: that the decision reaches the pill at
    // all. Both pills are in one row, which is the arrangement that makes a
    // fixed `color:` impossible — omacal shows Google's pale yellow beside its
    // dark blue, and a single foreground fails one of them.
    await page.goto(show('pill-inks'));
    const pills = page.locator('.pill');
    await expect(pills).toHaveCount(3);

    const [light, dark] = await resolveInks(page, ['var(--ink-on-light)', 'var(--ink-on-dark)']);
    // Without this the two assertions below could both hold with one ink.
    expect(light).not.toBe(dark);

    const ink = (n: number) => pills.nth(n).evaluate((el) => getComputedStyle(el).color);
    expect(await ink(0)).toBe(light); // #f6bf26, pale
    expect(await ink(1)).toBe(dark);  // #3f51b5, dark

    // …and that the fill really is the calendar's colour at full strength,
    // which is what makes the ink question exist at all. Under the old 16%
    // wash both pills' backgrounds were within a few percent of the theme's
    // own and this would read `rgba(…, 0.16)`.
    const fill = await pills.nth(0).evaluate((el) => getComputedStyle(el).backgroundColor);
    expect(fill).toBe('rgb(246, 191, 38)');
  });

  test('an unreadable colour is still readable', async ({ page }) => {
    // `ev.color` is non-nullable and `to_ui` only ever produces a hex, so this
    // is not a payload the backend sends — it is the guard on `foregroundFor`
    // being called during a render, where a throw takes the whole ribbon down
    // rather than one pill.
    //
    // The claim about *why* `var(--text)` is the right fallback is checked
    // here, not reasoned about: `background: var(--cal)` with a value the
    // browser cannot parse is invalid at computed-value time, so the
    // declaration drops to `transparent` and what shows behind the text is the
    // app's own background — which is exactly the surface `--text` is legible
    // on. If an engine ever resolved that differently the fallback would be
    // wrong, and this is what would say so.
    await page.goto(show('pill-inks'));
    const broken = page.locator('.pill').nth(2);
    await expect(broken).toHaveAttribute('title', 'Unreadable');

    const seen = await broken.evaluate((el) => ({
      color: getComputedStyle(el).color,
      background: getComputedStyle(el).backgroundColor,
    }));
    const [text] = await resolveInks(page, ['var(--text)']);
    // The fill the browser could not paint, and so the surface the text is
    // really sitting on: the app's own background, not the calendar's colour.
    expect(seen.background).toBe('rgba(0, 0, 0, 0)');
    expect(seen.color).toBe(text);
    // …and `--text` is not one of the two inks, so this is a genuinely
    // different branch rather than the light/dark decision landing somewhere
    // that happens to be legible.
    const [light, dark] = await resolveInks(page, ['var(--ink-on-light)', 'var(--ink-on-dark)']);
    expect([light, dark]).not.toContain(text);
  });

  test('a continuation edge stays visible against a solid fill', async ({ page }) => {
    // The dashed left edge is what says "this started earlier". It used to be
    // `border-left-style: dashed` over `border-left: 2px solid var(--cal)`,
    // which worked against a 16% wash and paints nothing at all against a fill
    // that *is* `--cal`: same colour, same colour, no marker. Asserted against
    // the pill's own background rather than against a named colour, so it
    // stays the right question if either end ever changes.
    await page.goto(show('crossing'));
    const tail = page.locator('.pill.cl');
    await expect(tail).toHaveCount(1);

    const edge = await tail.evaluate((el) => {
      const s = getComputedStyle(el);
      return { color: s.borderLeftColor, style: s.borderLeftStyle, background: s.backgroundColor };
    });
    expect(edge.style).toBe('dashed');
    expect(edge.color).not.toBe(edge.background);
  });

  // Task 10. The ribbon's day strip carried no click handler at all before
  // this, so it is the one grid where "empty space" is the whole day cell —
  // and the `z-index` pair that keeps the day number above the target is the
  // same shape `MonthGrid` needs, for the same reason.
  test('clicking a day asks the parent for a new event on it', async ({ page }) => {
    await page.goto(show('y2026'));
    // Row 0, column 4: four days after the ribbon's own anchor of Mon 29 Dec
    // 2025, so Fri 2 Jan 2026. No pill on it (row 0's runs 8-10) and not a
    // first-of-month (which would put a `.mchip` in the way), so nothing else
    // could have answered.
    //
    // Off-centre on purpose: a ribbon day is about 45px wide and 15px tall,
    // and its own number sits in the middle of it — the click has to land on
    // the part that is actually empty, which is also what proves the day
    // number is still on top rather than buried under the target.
    await page.locator('.rrow').first().locator('.rday .newhere').nth(4)
      .click({ position: { x: 3, y: 3 } });
    expect(await page.evaluate(() => (window as any).__lastCreate)).toMatchObject({
      startMs: Date.UTC(2026, 0, 2),
    });
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

    // The strip has to be tall enough to *hold* a day, which is a stronger
    // statement than "not zero" and the one that actually distinguishes the
    // two renderings: under the bug `.rdays` measured 0 while the `.rday`
    // inside it still measured its own 15px, so the days were being drawn
    // somewhere other than inside the strip that owns them. Read off the day
    // rather than written here as a number, so it stays true if the day's own
    // font or padding ever changes; and asserted *about* the strip, never
    // about the day, for the reason the comment above gives.
    //
    // This assertion used to read `|packed - quiet| < 1` — the packed row's
    // strip and a quiet row's within a pixel of each other — and that was
    // never a property of this layout. It held because `.rows` was pinned at
    // `calc(100vh - 190px)`, 530px at this suite's 720p viewport, which is
    // *less* than fourteen rows' own combined minimum (539px here). Every row
    // was therefore clamped to its minimum and the two agreed by force. Now
    // that `.rows` claims the height genuinely available to it (649px) the
    // quiet rows have room to grow into and the packed one, still held at its
    // 58px minimum by four lanes of pills, does not: 15px against 23.5px,
    // measured in both engines. Uniform row heights were the squeeze talking;
    // the day strip surviving a packed row is the property.
    const packedDays = (await packed.locator('.rdays').boundingBox())!.height;
    const oneDay = (await packed.locator('.rday').first().boundingBox())!.height;
    expect(oneDay).toBeGreaterThan(0); // guards the line below against 0 >= 0
    expect(packedDays).toBeGreaterThanOrEqual(oneDay);
  });

  test('all fourteen rows fit on one screen with no scroll', async ({ page }) => {
    // The design doc's own promise for this view (spec §4): "Big Year — one
    // screen, the whole year." `RESERVED_PILL_LANES`'s comment explains the
    // budget this depends on. Pinned so that budget can't drift back out of
    // reach without a spec noticing, the way the original 3-lane reservation
    // did.
    //
    // The container is 620px of `.rows` at the suite's default 1280x720
    // viewport (`devices['Desktop Chrome']`/`['Desktop Safari']` in
    // playwright.config.ts — no `setViewportSize` here, same as every other
    // spec in this file), against fourteen rows' own ~539px minimum. It used
    // to be 530px, which is *less* than that minimum: the ribbon was pinned at
    // `calc(100vh - 190px)` and this spec passed only because a fourteen-row
    // ribbon with one pill per row is a hair under what 530px holds. It now
    // reads the box `App` genuinely leaves a view, which
    // `app.spec.ts`'s "a standalone view gets the same box the app gives it"
    // pins to the real thing — so this is a claim about 720p in the app and
    // not only about the harness. Three reserved lanes rather than two would
    // still fail it, which is what it is for.
    await page.goto(show('y2026'));
    await expect(page.locator('.rrow')).toHaveCount(14);
    const overflow = await page.locator('.rows').evaluate((el) => el.scrollHeight - el.clientHeight);
    expect(overflow).toBeLessThanOrEqual(0);
  });

  // The reported defect, at the one place App's own specs cannot see it: its
  // `get_big_year` stub returns an empty legend, so the App-level height specs
  // in `app.spec.ts` only ever exercise a ribbon with nothing under its rows.
  // The legend is what made the old rule *look* nearly right — `100vh - 190px`
  // was 150px of guessed chrome plus 40px reserved for a legend that is not
  // 40px tall — and it is why the number the user reported (~95px short) was
  // smaller than the 123px this leaves with no legend at all. Nothing reserves
  // anything now: `.legend` takes what it needs, `.rows` takes the rest, and
  // the two together reach the bottom of the box the parent gave the ribbon.
  test('the rows take whatever the legend does not', async ({ page }) => {
    await page.goto(show('y2026'));
    await expect(page.locator('.legend .item')).toHaveCount(2);
    // Measured against the mount container rather than the window: this
    // component no longer knows anything about the window, which is the fix.
    const gap = await page.evaluate(() => {
      const box = document.getElementById('app')!.getBoundingClientRect();
      return box.bottom - document.querySelector('.legend')!.getBoundingClientRect().bottom;
    });
    // `.ribbon`'s own padding, and nothing else — read off the element so this
    // says "nothing but the padding" rather than "nothing but four pixels".
    const pad = await page.locator('.ribbon').evaluate(
      (el) => parseFloat(getComputedStyle(el).paddingBottom),
    );
    expect(gap).toBeCloseTo(pad, 0);
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

test.describe('EventForm', () => {
  /**
   * Navigate with the clock frozen, every time.
   *
   * Not optional and not per-spec: the `create` fixture is built from
   * `Date.now()` inside the page (see fixtures.ts), because the "next half
   * hour" default is the thing under test and a fixture that pinned the
   * instant itself could not tell a form that applies the default from one
   * that was handed the answer. Freezing it here means no spec in this block
   * can forget, and none of them rots into a failure on a future date.
   */
  const open = async (page: import('@playwright/test').Page, fixture: string) => {
    await page.clock.setFixedTime(FORM_NOW);
    await page.goto(`/tests/harness/index.html?c=EventForm&f=${fixture}`);
    await expect(page.locator('.pop')).toBeVisible();
  };

  /** Everything the form handed `onsave`, in order — `[]` when it refused.
   *  An array, not a slot: half of what these specs assert is that nothing
   *  was saved at all. */
  const saves = (page: import('@playwright/test').Page) =>
    page.evaluate(() => (window as any).__saves as any[]);

  test('only writable calendars are offered', async ({ page }) => {
    // A subscribed holiday calendar is a `reader` and a room is a
    // `freeBusyReader`; `create_impl` refuses both server-side, so offering
    // either produces a Save that can only fail. Two unwritable roles, not
    // one, so a filter written as "anything but reader" is caught too.
    await open(page, 'create');
    const select = page.getByLabel('Calendar', { exact: true });
    await expect(select.locator('option')).toHaveCount(2);
    await expect(select.locator('option')).toHaveText(['Personal', 'Team']);
    for (const name of FORM_UNWRITABLE_NAMES) {
      await expect(select.locator('option').filter({ hasText: name })).toHaveCount(0);
    }
  });

  test('a create seeded with a calendar it cannot write to falls back to one it can', async ({ page }) => {
    // Filtering the option list is not filtering the value. Seeded with the
    // reader's id, the select rendered *blank* — no option matches — and Save
    // then sent that id with nothing on screen to say so. Task 10 chooses this
    // seed, so the shape is reachable from the next task rather than theoretical.
    await open(page, 'create-seeded-unwritable');
    const select = page.getByLabel('Calendar', { exact: true });
    await expect(select).not.toHaveValue('');
    await expect(select).toHaveValue(String(FORM_FALLBACK_ID));

    await page.getByRole('button', { name: 'Create' }).click();
    const [saved] = await saves(page);
    // What is shown and what is saved have to agree; that is the property that
    // was broken, so both halves are asserted.
    expect(saved.calendarId).toBe(FORM_FALLBACK_ID);
    expect(saved.calendarId).not.toBe(FORM_UNWRITABLE_ID);
  });

  test('the calendar can be chosen on a create and not on an edit', async ({ page }) => {
    // `update_event` takes no calendar id — it reads the target from
    // `event_for_write(id)` — so an enabled control on an edit silently
    // discards the choice. Both arms in one spec: `disabled={true}` always
    // would pass the edit half on its own.
    await open(page, 'create');
    await expect(page.getByLabel('Calendar', { exact: true })).toBeEnabled();

    await open(page, 'with-guests');
    await expect(page.getByLabel('Calendar', { exact: true })).toBeDisabled();
  });

  test('moving the start date takes the end date with it', async ({ page }) => {
    // Otherwise changing the date of an ordinary one-hour meeting leaves the
    // end date on the old day and Save refuses a range the user never asked
    // for. Asserted through to the saved instants, not just the input: the
    // point is that the save is *accepted* and lands on the new day.
    await open(page, 'create');
    await page.getByLabel('Date', { exact: true }).fill('2026-08-12');
    await expect(page.getByLabel('End date', { exact: true })).toHaveValue('2026-08-12');

    await page.getByRole('button', { name: 'Create' }).click();
    const [saved] = await saves(page);
    // 09:30 on 12 Aug 2026, UTC — the project's `timezoneId`.
    expect(saved.fields.when.kind).toBe('timed');
    expect(saved.fields.when.startMs).toBe(Date.UTC(2026, 7, 12, 9, 30));
    expect(saved.fields.when.endMs).toBe(Date.UTC(2026, 7, 12, 10, 0));
  });

  test('save is refused when the end is before the start', async ({ page }) => {
    // Refused, not corrected. Silently swapping the ends would save something
    // nobody asked for, and on an event with guests mail it to all of them.
    await open(page, 'end-before-start');
    await page.getByRole('button', { name: 'Create' }).click();
    // Nothing saved, asserted first: that is the safety property, and it is
    // the one whose failure names the actual defect. Telling the user why
    // matters too, but a form that saved a backwards event and then apologised
    // would still have saved it.
    expect(await saves(page)).toEqual([]);
    await expect(page.getByTestId('form-error')).toBeVisible();
  });

  test('an unrepresentable repeat rule is shown as a disabled Custom option', async ({ page }) => {
    // Spec §6's UI half. `write::repeat_from_rrule` answered `custom` for this
    // fortnightly rule, and the form's job is to show what it cannot rewrite
    // rather than quietly present it as something it can.
    await open(page, 'custom-repeat');
    const select = page.getByLabel('Repeat', { exact: true });
    await expect(select).toHaveValue('custom');

    const custom = select.locator('option[value="custom"]');
    // The *entry* is disabled; the select is not. Disabling the select would
    // make the rule unchangeable rather than un-clobberable, and the whole
    // design is that replacing it stays possible as an explicit act.
    //
    // Asserted through the DOM property rather than `toBeDisabled()`, which
    // resolves disabledness through the ARIA state and reports an `<option>`
    // carrying a real `disabled` attribute as enabled. The property is what
    // actually makes the entry unselectable, so it is what is worth asserting.
    expect(await custom.evaluate((el) => (el as HTMLOptionElement).disabled)).toBe(true);
    await expect(select.locator('option:not([disabled])')).toHaveCount(6);
    await expect(select).toBeEnabled();
    // In words, not as the raw rule: `RRULE:FREQ=WEEKLY;INTERVAL=2` is not
    // something to ask a user to read before deciding whether to replace it.
    await expect(custom).toHaveText('Custom · Every 2 weeks');
  });

  test('an event with guests warns that saving notifies them', async ({ page }) => {
    // `patch_event` sends `sendUpdates=all` unconditionally, so every save on
    // this event is also five emails — four, once the person doing the saving
    // is taken out of the count. The fixture has five attendees for exactly
    // that reason.
    await open(page, 'with-guests');
    await expect(page.getByTestId('guest-notice')).toHaveText('Saving will notify 4 guests.');
  });

  test('a description is rendered as text, never as markup', async ({ page }) => {
    // Anyone who knows the user's email can put an event on their calendar,
    // description included, and this webview can invoke Tauri commands.
    await open(page, 'nasty-description');
    await expect(page.locator('img')).toHaveCount(0);
    // And byte for byte: sanitising on the way *in* would rewrite what the
    // author typed and then save the rewrite back over the real event —
    // `stripTags` alone would leave this field empty.
    await expect(page.getByLabel('Description', { exact: true }))
      .toHaveValue('<img src=x onerror=alert(1)>');
  });

  test('a new event opens at the next half hour', async ({ page }) => {
    // 09:12 frozen, so 09:30 is a rounding rather than an echo of the clock.
    await open(page, 'create');
    await expect(page.getByLabel('Date', { exact: true })).toHaveValue('2026-08-05');
    await expect(page.getByLabel('Start', { exact: true })).toHaveValue('09:30');
    await expect(page.getByLabel('End', { exact: true })).toHaveValue('10:00');
  });

  test('a recurring edit offers three scopes and says what All events does', async ({ page }) => {
    // "All events" on a time change shifts the whole series rather than
    // pinning every occurrence to the edited date — deliberate (the
    // alternative drops occurrences before the clicked one) and impossible to
    // infer from three radio labels.
    await open(page, 'recurring-edit');
    await expect(page.getByRole('radio')).toHaveCount(3);
    await expect(page.getByRole('radio', { name: 'This event' })).toBeChecked();
    await expect(page.getByTestId('all-events-note')).toHaveCount(0);

    await page.getByRole('radio', { name: 'All events' }).check();
    await expect(page.getByTestId('all-events-note')).toContainText('every occurrence an hour later');
  });

  test('a one-off event offers no scope choice', async ({ page }) => {
    // Without this the scope spec above passes on a form that always shows
    // three radios, whatever it was given.
    await open(page, 'with-guests');
    await expect(page.getByRole('radio')).toHaveCount(0);
  });

  test('a multi-day all-day event keeps its last day, and saves it back unchanged', async ({ page }) => {
    // Google's `end.date` is exclusive and so is the store's `end_ms`: a
    // three-day trip starting Mon 10 Aug ends at midnight on Thu 13th. Showing
    // that date reads a day long; sending back the date shown shortens the trip
    // by a day and mails everyone about it. Both ends are asserted, because
    // converting on only one side is the failure that looks right on screen.
    await open(page, 'multi-day-all-day');
    await expect(page.getByLabel('First day', { exact: true })).toHaveValue(TRIP_FIRST_DAY);
    await expect(page.getByLabel('Last day', { exact: true })).toHaveValue(TRIP_LAST_DAY);

    await page.getByRole('button', { name: 'Save' }).click();
    const [saved] = await saves(page);
    expect(saved.fields.when.kind).toBe('allDay');
    // Still the exclusive end, now named as the date it always was. Playwright
    // pins `timezoneId: 'UTC'`, so this is the same assertion in a different
    // unit — which is exactly why the zone-crossing version of it belongs in a
    // describe of its own, and does not exist yet.
    expect(saved.fields.when.endDate).toBe(TRIP_END_DATE);
  });

  test('saving without touching Repeat sends no rule at all', async ({ page }) => {
    // The property the whole `custom` design rests on: an absent `repeat` means
    // "the user did not touch Repeat", and the existing rule is left alone.
    // Sending `custom` — or anything else — would rewrite a fortnightly meeting
    // as something omacal can express, for the whole guest list.
    await open(page, 'custom-repeat');
    await page.getByRole('button', { name: 'Save' }).click();
    const [saved] = await saves(page);
    expect(Object.keys(saved.fields)).not.toContain('repeat');
  });

  test('choosing another repeat option sends the overwrite explicitly', async ({ page }) => {
    // The other half: leaving an untouched rule alone must not turn into never
    // being able to change one.
    await open(page, 'custom-repeat');
    await page.getByLabel('Repeat', { exact: true }).selectOption('weekly');
    await page.getByRole('button', { name: 'Save' }).click();
    const [saved] = await saves(page);
    expect(saved.fields.repeat).toBe('weekly');
  });
});

test.describe('DeleteConfirm', () => {
  const show = (f: string) => `/tests/harness/index.html?c=DeleteConfirm&f=${f}`;

  const open = async (page: import('@playwright/test').Page, fixture: string) => {
    await page.goto(show(fixture));
    await expect(page.locator('.pop')).toBeVisible();
  };

  /** Every scope the panel handed `onconfirm`, in order — `[]` when it asked
   *  and nothing was confirmed. An array, not a slot, for the reason
   *  `EventForm`'s `__saves` is one: half of what these assert is that nothing
   *  happened at all. */
  const confirms = (page: import('@playwright/test').Page) =>
    page.evaluate(() => (window as any).__confirms as any[]);

  test('names the event, says who gets emailed, and warns there is no undo', async ({ page }) => {
    // Three things a confirmation with no undo behind it has to be honest
    // about, and one it must not be: the fixture has three attendees, one of
    // them the signed-in user, so the count is two. Telling somebody they are
    // about to email themselves is just wrong.
    await open(page, 'one-off');
    await expect(page.locator('h2')).toContainText('Board prep');
    await expect(page.getByTestId('delete-guest-notice')).toContainText('2 guests are told by email');
    await expect(page.getByTestId('delete-no-undo')).toContainText('cannot be undone');
    // A one-off has one deletion, so naming it three ways would be three
    // different words for the same act.
    await expect(page.getByRole('radio')).toHaveCount(0);
    expect(await confirms(page)).toEqual([]);
  });

  test('the three scopes are three different operations, and each says which', async ({ page }) => {
    // Not three sizes of one deletion. "This and following" deletes nothing at
    // all — it patches the series' rule so it stops earlier, which is the only
    // way to lose the tail without also losing the occurrences before the
    // clicked one, since they are all the same Google event. "All events" takes
    // the past with it. Neither is inferable from a three-item radio list.
    await open(page, 'recurring');
    const scopes = page.locator('.scope label');
    await expect(scopes).toHaveCount(3);
    await expect(scopes.nth(1)).toContainText('deletes nothing');
    await expect(scopes.nth(1)).toContainText('shortens the series');
    await expect(scopes.nth(2)).toContainText('already happened');
    // Every scope notifies: `sendUpdates=all` is unconditional on the DELETE
    // and on the "this and following" PATCH alike, so the notice may not read
    // as if it applied to only one of the three.
    await expect(page.getByTestId('delete-guest-notice')).toContainText('Whichever you choose');

    // And the chosen scope is the one that comes back — a panel that always
    // confirmed `'this'` would satisfy every assertion above.
    await page.getByRole('radio', { name: 'This and following' }).check();
    await page.getByRole('button', { name: 'Delete' }).click();
    expect(await confirms(page)).toEqual(['following']);
  });

  // Each radio bound to the scope it actually sends, one spec per option, so
  // that no option can be silently rewired to another. The two that are not
  // the default matter most and differ most: "All events" removes a whole
  // series *including its past*, "This and following" removes nothing at all
  // and merely shortens the rule. Wiring the first to the second leaves the
  // panel reading exactly right and is a different, irreversible act — with
  // mail going out either way. Only an assertion per option catches it.
  for (const [label, scope] of [
    ['This event', 'this'],
    ['This and following', 'following'],
    ['All events', 'all'],
  ] as const) {
    test(`"${label}" sends the scope ${scope}`, async ({ page }) => {
      await open(page, 'recurring');
      await page.getByRole('radio', { name: label }).check();
      await page.getByRole('button', { name: 'Delete' }).click();
      expect(await confirms(page)).toEqual([scope]);
    });
  }

  test('an event with nobody on it claims nothing about guests', async ({ page }) => {
    // "0 guests are told by email" is both untrue and alarming. The no-undo
    // line stays either way: that one is about the event, not the guest list.
    await open(page, 'no-guests');
    await expect(page.getByTestId('delete-guest-notice')).toHaveCount(0);
    await expect(page.getByTestId('delete-no-undo')).toBeVisible();
  });
});
