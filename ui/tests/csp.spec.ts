import { test, expect } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const app = () => '/tests/harness/index.html?c=App&f=default';

/**
 * The harness runs the whole suite under the app's Content-Security-Policy
 * (tests/harness/index.html), so a new inline script, remote image or eval
 * fails a test here rather than blanking a user's window. This is the one
 * place the two copies are held together: the harness's may differ from
 * tauri.conf.json's only by Vite's HMR socket, and the dev policy only by
 * the dev server's.
 */
const IPC = "connect-src 'self' ipc: http://ipc.localhost;";

test("the harness runs under the app's own Content-Security-Policy", async ({ page }) => {
  const conf = JSON.parse(readFileSync(
    fileURLToPath(new URL('../../src-tauri/tauri.conf.json', import.meta.url)), 'utf8',
  ));
  const app: string = conf.app.security.csp;
  expect(app, 'the app policy names the IPC origins the backend answers on').toContain(IPC);
  expect(conf.app.security.devCsp).toBe(app.replace(IPC, IPC.replace(';', ' ws://localhost:1420;')));

  await page.goto('/tests/harness/index.html');
  const meta = page.locator('meta[http-equiv="Content-Security-Policy"]');
  await expect(meta).toHaveCount(1);
  expect(await meta.getAttribute('content')).toBe(app.replace(IPC, IPC.replace(';', ' ws://localhost:5199;')));
});

/**
 * A violation is not an error the suite would otherwise see: a blocked
 * stylesheet or socket degrades the page without failing an assertion. The
 * document reports each one as an event, so the app page is loaded and
 * driven a little with a listener in place, and the list must stay empty.
 */
test('the app page raises no policy violation', async ({ page }) => {
  await page.addInitScript(() => {
    (window as any).__cspViolations = [];
    document.addEventListener('securitypolicyviolation', (e) => {
      (window as any).__cspViolations.push(`${e.violatedDirective} ${e.blockedURI}`);
    });
  });
  await page.goto(app());
  await expect(page.locator('.ev').first()).toBeVisible();
  await page.locator('.ev').first().click();
  await page.getByRole('button', { name: 'Menu' }).click();
  await page.getByRole('button', { name: 'Settings…' }).click();
  await expect(page.getByRole('dialog', { name: 'Settings' })).toBeVisible();
  expect(await page.evaluate(() => (window as any).__cspViolations)).toEqual([]);
});
