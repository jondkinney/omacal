import { test, expect, type Page } from '@playwright/test';
import { APP_ONE_OFF_ID, POPOVER_DETAILS } from './fixtures';

/** Issue #109: left-click keeps the details card, right-click goes straight
 *  to the editor. The card is the thing that must NOT appear — every case
 *  below asserts its absence rather than only the editor's presence, because
 *  "the editor opened" is also true of a right-click that opened the card
 *  first and then the editor a frame later, which is the flicker the feature
 *  exists to avoid. */

const editor = (page: Page) => page.getByRole('dialog', { name: 'Edit event', exact: true });
const card = (page: Page) => page.getByRole('dialog', { name: 'Board prep', exact: true });

async function week(page: Page) {
  await page.goto('/tests/harness/index.html?c=App&f=writable');
  const event = page.locator('.ev').filter({ hasText: 'Board prep' });
  await event.scrollIntoViewIfNeeded();
  await expect(event).toBeVisible();
  return event;
}

test('right-clicking an event in Week opens the editor, never the details card', async ({ page }) => {
  const event = await week(page);
  await event.click({ button: 'right' });

  await expect(editor(page)).toBeVisible();
  await expect(card(page)).toHaveCount(0);
  await expect(editor(page).getByLabel('Title')).toHaveValue('Board prep');
});

test('left-clicking the same event still opens the details card', async ({ page }) => {
  const event = await week(page);
  await event.click();

  await expect(card(page)).toBeVisible();
  await expect(editor(page)).toHaveCount(0);
});

/** The threshold rule the grid's own right-click already keeps: `contextmenu`
 *  fires at the press, before the gesture has a shape, so the press is only
 *  remembered and the release decides. A right press that travels is not a
 *  right-click — and it must not have started a drag either. */
test('a right press that travels edits nothing and moves nothing', async ({ page }) => {
  const event = await week(page);
  const box = (await event.boundingBox())!;
  const x = box.x + box.width / 2;
  const y = box.y + box.height / 2;

  await page.mouse.move(x, y);
  await page.mouse.down({ button: 'right' });
  await page.mouse.move(x, y + 120, { steps: 8 });
  await page.mouse.up({ button: 'right' });

  await expect(editor(page)).toHaveCount(0);
  await expect(card(page)).toHaveCount(0);
  const writes = await page.evaluate(() => window.__harness.calls
    .filter((c) => c.cmd === 'update_event' || c.cmd === 'move_event'));
  expect(writes).toHaveLength(0);
});

/** The browser's own Reload / Back / View Source menu is chrome inside what
 *  presents itself as a native app, and the block now has a meaning for that
 *  button of its own. */
test('the webview context menu never appears on an event', async ({ page }) => {
  const event = await week(page);
  const defaultPrevented = await event.evaluate((el) => {
    const e = new MouseEvent('contextmenu', { bubbles: true, cancelable: true });
    el.dispatchEvent(e);
    return e.defaultPrevented;
  });
  expect(defaultPrevented).toBe(true);
});

/** `can_edit` gates this exactly as it gates the card's own Edit button: an
 *  editor opened on a subscribed holiday calendar could only produce a Save
 *  the server refuses. Falling through to the card beats answering nothing —
 *  a control that does nothing reads as broken, which this project has had
 *  reported against it before. */
test('a right-click on an event that cannot be edited opens the card instead', async ({ page }) => {
  const event = await week(page);
  await page.evaluate((id) => window.__harness.holdNextEventCall('event_detail', id), APP_ONE_OFF_ID);
  await event.click({ button: 'right' });
  await page.evaluate(({ id, detail }) => window.__harness.releaseEventCall('event_detail', id, detail),
    { id: APP_ONE_OFF_ID, detail: { ...POPOVER_DETAILS[APP_ONE_OFF_ID], can_edit: false } });

  await expect(card(page)).toBeVisible();
  await expect(editor(page)).toHaveCount(0);
});

/** Month's chips are their own markup rather than `EventBlock`s, so the two
 *  surfaces cannot share a test any more than they share a component. Both
 *  chip kinds — the multi-day bar and the timed line — are checked, because
 *  they are two separate elements in `MonthGrid` and wiring one is no proof
 *  of the other. */
test.describe('Month chips', () => {
  const show = (f: string) => `/tests/harness/index.html?c=MonthGrid&f=${f}`;
  const lastEdit = (page: Page) => page.evaluate(() => (window as any).__lastEdit);
  const lastOpen = (page: Page) => page.evaluate(() => (window as any).__lastOpen);

  test('right-clicking a timed chip asks the parent to edit, not to open', async ({ page }) => {
    await page.goto(show('august'));
    await page.locator('.mcell .timed').first().click({ button: 'right' });
    expect((await lastEdit(page))?.event?.title).toBe('Standup');
    expect(await lastOpen(page)).toBeFalsy();
  });

  test('right-clicking a multi-day bar does the same', async ({ page }) => {
    await page.goto(show('august'));
    await page.locator('.bar', { hasText: 'Berlin trip' }).first().click({ button: 'right' });
    expect((await lastEdit(page))?.event?.title).toBe('Berlin trip');
    expect(await lastOpen(page)).toBeFalsy();
  });

  test('a left click still opens', async ({ page }) => {
    await page.goto(show('august'));
    await page.locator('.mcell .timed').first().click();
    expect((await lastOpen(page))?.event?.title).toBe('Standup');
    expect(await lastEdit(page)).toBeFalsy();
  });
});
