import { test, expect, type Page } from '@playwright/test';

/**
 * The menu-bar popup's own window — `index.html?menubar`, the root
 * `main.ts` mounts for it. It is the macOS popup, but it is a webview like
 * any other, so its layout is measurable here rather than only on a Mac.
 *
 * Nothing stubs Tauri on this route, so the agenda stays on its loading
 * line. That is fine and deliberate: what these cases are about is the
 * header, which renders before any data and is where the reported problem
 * was.
 */
const open = async (page: Page) => {
  await page.setViewportSize({ width: 420, height: 700 });
  await page.goto('/index.html?menubar');
  await expect(page.locator('header strong')).toHaveText('OmaCal');
};

const icons = (page: Page) => page.locator('header .acts .icon');

test('the three header actions sit together at the right, not spread across the popup', async ({ page }) => {
  await open(page);
  const header = (await page.locator('header').boundingBox())!;
  const boxes = await icons(page).evaluateAll((els) =>
    els.map((el) => el.getBoundingClientRect()).map((r) => ({ left: r.left, right: r.right })));
  expect(boxes).toHaveLength(3);

  // The reported shape: `space-between` across four children put the first
  // button near the middle of the popup. Grouped, the whole run of three is
  // in the right-hand third and the gaps between them are small.
  const run = boxes[2].right - boxes[0].left;
  expect(run, 'the three actions should occupy a compact run, not the whole header')
    .toBeLessThan(header.width / 2);
  expect(boxes[0].left, 'and that run should sit in the right-hand half')
    .toBeGreaterThan(header.x + header.width / 2);
  expect(Math.round(boxes[2].right), 'the last one ends at the header’s right edge')
    .toBeLessThanOrEqual(Math.round(header.x + header.width) + 1);
});

test('the header actions are one size, not three', async ({ page }) => {
  await open(page);
  const sizes = await icons(page).evaluateAll((els) =>
    els.map((el) => el.getBoundingClientRect()).map((r) => `${Math.round(r.width)}x${Math.round(r.height)}`));
  // `+`, `⚙` and `×` have very different optical weights; a shared box is
  // what makes them read as one row of controls.
  expect(new Set(sizes).size, `the three targets differ in size: ${sizes.join(', ')}`).toBe(1);
  const [w, h] = sizes[0].split('x').map(Number);
  expect(w, 'and each is a real target rather than a bare glyph').toBeGreaterThanOrEqual(28);
  expect(h).toBeGreaterThanOrEqual(28);
});

test('every header action still says what it is', async ({ page }) => {
  await open(page);
  for (const name of ['Add event', 'Open preferences', 'Close agenda']) {
    await expect(page.getByRole('button', { name, exact: true })).toBeVisible();
  }
});
