import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: './tests',
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
