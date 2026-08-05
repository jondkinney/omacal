import { test, expect } from '@playwright/test';
import { FIXED_NOW } from './fixtures';

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
    // Demo mode alone does not imply a connected account.
    await expect(page.getByRole('button', { name: 'Connect Google Calendar' })).toBeVisible();
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
});
