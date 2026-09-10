import { test, expect, type Page } from '@playwright/test';

async function start(page: Page) {
  await page.goto('/tests/harness/index.html?c=App&f=writable');
  const event = page.locator('.ev').filter({ hasText: 'Board prep' });
  await event.scrollIntoViewIfNeeded();
  const eventBox = (await event.boundingBox())!;
  const col = await event.evaluate((el) => {
    const col = el.closest<HTMLElement>('.col')!;
    const r = col.getBoundingClientRect();
    return { x: r.x, y: r.y, width: r.width, height: r.height };
  });
  // Start on the resize edge to prove Alt overrides resizing as well as moving.
  const x = eventBox.x + eventBox.width / 2;
  const y = eventBox.y + 2;
  await page.keyboard.down('Alt');
  await page.mouse.move(x, y);
  await page.mouse.down();
  return { event, eventBox, col, x, y };
}

const writes = (page: Page) => page.evaluate(() => window.__harness.calls
  .filter((c) => c.cmd === 'update_event' || c.cmd === 'create_event'));

test('Alt-drag over an event creates a blank draft and stays in create mode after Alt is released', async ({ page }) => {
  const { event, eventBox, col, x, y } = await start(page);
  await page.mouse.move(x, y + col.height / 24, { steps: 5 });
  await expect(page.locator('.sweep')).toBeVisible();
  await page.keyboard.up('Alt');
  await page.mouse.move(x, y + col.height / 12, { steps: 5 });
  await page.mouse.up();
  const form = page.getByRole('dialog', { name: 'New event', exact: true });
  await expect(form).toBeVisible();
  await expect(form.getByLabel('Title', { exact: true })).toHaveValue('');
  const startQuarter = Math.round((y - col.y) / col.height * 96);
  const clock = (q: number) => `${String(Math.floor(q / 4)).padStart(2, '0')}:${String(q % 4 * 15).padStart(2, '0')}`;
  await expect(form.getByLabel('Start', { exact: true })).toHaveValue(clock(startQuarter));
  await expect(form.getByLabel('End', { exact: true })).toHaveValue(clock(startQuarter + 8));
  expect(await writes(page)).toEqual([]);
  await page.keyboard.press('Escape');
  await expect(form).toHaveCount(0);
  expect((await event.boundingBox())!.y).toBeCloseTo(eventBox.y, 1);
  expect((await event.boundingBox())!.height).toBeCloseTo(eventBox.height, 1);
});

test('Alt-drag can start on an event and sweep across days', async ({ page }) => {
  const { col, x, y } = await start(page);
  await page.mouse.move(x + col.width, y + 12, { steps: 6 });
  await expect(page.getByRole('status', { name: 'New event duration' }).locator('strong')).toHaveText('2 days');
  await page.mouse.up();
  await page.keyboard.up('Alt');
  const form = page.getByRole('dialog', { name: 'New event', exact: true });
  await expect(form).toBeVisible();
  await expect(form.getByLabel('All day', { exact: true })).toBeChecked();
  await expect(form.getByLabel('First day', { exact: true })).toHaveValue('2024-01-29');
  await expect(form.getByLabel('Last day', { exact: true })).toHaveValue('2024-01-30');
  expect(await writes(page)).toEqual([]);
});

test('Escape cancels an Alt-drag without opening a draft or moving the source', async ({ page }) => {
  const { x, y } = await start(page);
  await page.mouse.move(x, y + 50, { steps: 5 });
  await expect(page.locator('.sweep')).toBeVisible();
  await page.keyboard.press('Escape');
  await page.mouse.up();
  await page.keyboard.up('Alt');
  await expect(page.locator('.sweep')).toHaveCount(0);
  await expect(page.getByRole('dialog')).toHaveCount(0);
  expect(await writes(page)).toEqual([]);
});

test('Alt changes a stationary hover from resize to crosshair, and resets on release or blur', async ({ page }) => {
  await page.goto('/tests/harness/index.html?c=App&f=writable');
  const event = page.locator('.ev').filter({ hasText: 'Board prep' });
  await event.scrollIntoViewIfNeeded();
  const box = (await event.boundingBox())!;
  const point = { x: box.x + box.width / 2, y: box.y + 2 };
  await page.mouse.move(point.x, point.y);
  const cursor = () => page.evaluate(({ x, y }) => getComputedStyle(document.elementFromPoint(x, y)!).cursor, point);
  await expect.poll(cursor).toBe('ns-resize');
  await page.keyboard.down('Alt');
  await expect.poll(cursor).toBe('crosshair');
  await expect(event.locator('.grip')).toHaveCount(0);
  await expect(event.locator('.tip')).toHaveCount(0);
  await page.keyboard.up('Alt');
  await expect.poll(cursor).toBe('ns-resize');
  await page.keyboard.down('Alt');
  await expect.poll(cursor).toBe('crosshair');
  await page.evaluate(() => window.dispatchEvent(new Event('blur')));
  await expect.poll(cursor).toBe('ns-resize');
  await page.keyboard.up('Alt');
  expect(await writes(page)).toEqual([]);
});

test('the new-event preview paints above the existing event under the pointer', async ({ page }) => {
  const { x } = await start(page);
  const covered = page.locator('.ev').filter({ hasText: 'Client call' });
  const box = (await covered.boundingBox())!;
  await page.mouse.move(x, box.y + box.height / 2, { steps: 6 });
  const ghost = page.locator('.sweep');
  await expect(ghost).toBeVisible();
  const paintsOnTop = await ghost.evaluate((el, targetBox) => {
    const g = el.getBoundingClientRect();
    const y = (Math.max(g.top, targetBox.y) + Math.min(g.bottom, targetBox.y + targetBox.height)) / 2;
    const x = g.left + g.width / 2;
    // Hit-testing follows paint order. Temporarily include the passive preview
    // in it without changing its position, opacity or stacking order.
    const old = el.style.pointerEvents;
    el.style.pointerEvents = 'auto';
    const hit = document.elementFromPoint(x, y);
    el.style.pointerEvents = old;
    return hit === el || el.contains(hit);
  }, box);
  expect(paintsOnTop).toBe(true);
  await expect(page.locator('.ev .tip')).toHaveCount(0);
  await page.keyboard.press('Escape');
  await page.mouse.up();
  await page.keyboard.up('Alt');
  expect(await writes(page)).toEqual([]);
});

test('drag feedback shows the snapped duration live, including reverse drags', async ({ page }) => {
  const { x, y, col } = await start(page);
  const feedback = page.getByRole('status', { name: 'New event duration' });
  await expect(feedback).toHaveCount(0);
  for (const [hours, duration] of [[0.5, '30 min'], [1, '1h'], [1.25, '1h 15m'], [-0.5, '30 min']] as const) {
    await page.mouse.move(x, y + col.height / 24 * hours, { steps: 5 });
    await expect(feedback.locator('strong')).toHaveText(duration);
  }
  await page.mouse.up();
  await page.keyboard.up('Alt');
  await expect(feedback).toHaveCount(0);
  await expect(page.getByRole('dialog', { name: 'New event', exact: true })).toBeVisible();
  expect(await writes(page)).toEqual([]);
});
