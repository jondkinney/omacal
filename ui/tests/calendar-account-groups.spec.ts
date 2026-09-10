import { test, expect } from '@playwright/test';
import { FORM_NOW } from './fixtures';

const headings = ['Google · shared@example.com', 'CalDAV · shared@example.com', 'CalDAV · shared@example.com'];

for (const host of ['settings', 'popover']) {
  test(`${host} separates account identities sharing an email and names their integrations`, async ({ page }) => {
    await page.goto(`/tests/harness/index.html?c=${host === 'settings' ? 'Header' : 'CalendarPopover'}&f=shared-email-accounts`);
    if (host === 'settings') {
      await page.getByRole('button', { name: 'Menu', exact: true }).click();
      await page.getByRole('button', { name: 'Settings…' }).click();
      await page.getByRole('tab', { name: 'Calendars', exact: true }).click();
    } else {
      await page.getByRole('button', { name: /Calendars/ }).click();
    }
    await expect(page.locator('.acct')).toHaveText(headings);
    // The backend can interleave calendars when sorting their names. Each
    // account must keep its own rows together, including repeated names.
    const groups = await page.locator('.acct').evaluateAll(els => els.map(el => {
      const names: string[] = [];
      for (let row = el.nextElementSibling; row && !row.classList.contains('acct'); row = row.nextElementSibling) {
        if (row.classList.contains('row')) names.push(row.querySelector('.name')!.textContent!);
      }
      return names;
    }));
    expect(groups).toEqual([['Personal', 'Work'], ['Personal'], ['Personal']]);
    // An action on the second account's identically named calendar still
    // addresses that calendar alone.
    await page.locator('.list > .row').nth(2).getByRole('checkbox').uncheck();
    const calls = await page.evaluate(() => (window as any).__harness.calls.filter((c: any) => c.cmd === 'set_calendar_selected'));
    expect(calls).toHaveLength(1);
    expect(calls[0].args).toEqual({ id: 201, on: false });
  });
}

test('the event calendar picker separates integrations and selects the intended same-name calendar', async ({ page }) => {
  await page.clock.setFixedTime(FORM_NOW);
  await page.goto('/tests/harness/index.html?c=EventForm&f=shared-email-accounts');
  await page.getByLabel('Title', { exact: true }).fill('Account grouping test');
  await page.getByRole('button', { name: 'Calendar', exact: true }).click();
  const list = page.getByRole('listbox', { name: 'Calendar', exact: true });
  await expect(list.locator('.acct')).toHaveText(headings);
  await expect(list.locator('.name')).toHaveText(['Personal', 'Work', 'Personal', 'Personal']);
  await list.getByRole('option', { name: 'Personal', exact: true }).last().click();
  await page.getByRole('button', { name: 'Create', exact: true }).click();
  const saves = await page.evaluate(() => (window as any).__saves);
  expect(saves).toHaveLength(1);
  expect(saves[0].calendarId).toBe(301);
});
