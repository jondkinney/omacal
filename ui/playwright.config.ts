import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: './tests',
  // Not for speed (though it is ~6x faster): without it, every test in a file
  // runs in one worker, and the 64th browser context that worker opens hangs
  // on `page.goto` — the navigation never fires `load`, and never recovers,
  // even given three minutes. Reproduced with a throwaway spec of 70 identical
  // `goto`s and no application code at all: probe 63 hung, probes 64-69 passed.
  // It is a WebKit-under-Playwright artifact, not anything this app does, but
  // `components.spec.ts` had grown to exactly 63 tests, so the next test added
  // to it — any test — was the one that hung. Spreading tests across workers
  // keeps each worker's context count an order of magnitude below the ceiling.
  //
  // Tests here are already independent: each does its own `page.goto` and gets
  // its own page, and all shared state (the fixtures, the Tauri stub) is
  // per-page.
  fullyParallel: true,
  // Snapshots are the point of this suite; a stale one must fail, not silently update.
  updateSnapshots: 'missing',
  expect: { toHaveScreenshot: { maxDiffPixelRatio: 0.01 } },
  // Fixtures place events by a literal top-fraction of the day (mins / 1440),
  // while WeekGrid's hour gridlines derive from the *local* wall-clock hour
  // of day.start_ms (see WeekGrid's hourFrac). Fixture timestamps are UTC
  // instants, so those two only agree when the browser's local zone is UTC
  // too — otherwise gridlines and events drift apart by the host's offset.
  use: { baseURL: 'http://localhost:5199', timezoneId: 'UTC', locale: 'en-US' },
  webServer: {
    command: 'npx vite --port 5199 --strictPort',
    url: 'http://localhost:5199/tests/harness/index.html',
    reuseExistingServer: !process.env.CI,
  },
  projects: [
    // WebKit first: closest available engine to the WebKitGTK the Linux target uses.
    { name: 'webkit', use: { ...devices['Desktop Safari'] } },
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
  ],
});
