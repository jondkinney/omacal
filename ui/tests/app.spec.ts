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
