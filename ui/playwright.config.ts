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
  // keeps each worker's context count under the ceiling.
  //
  // Tests here are already independent: each does its own `page.goto` and gets
  // its own page, and all shared state (the fixtures, the Tauri stub) is
  // per-page.
  fullyParallel: true,
  // …but only if there are enough workers to spread them across, and that is
  // not something to leave to the host. Playwright defaults `workers` to half
  // the logical core count, so a 2-vCPU CI runner gets *one* — every test in
  // the project lands in a single process, which passes the ceiling at test 64
  // and stalls there, surfacing as a 30-second timeout inside whatever
  // unrelated test happens to occupy that slot. Nothing about the failing test
  // is the cause, which is what makes it expensive to diagnose.
  //
  // The margin the default leaves is also thinner than "spread them out"
  // suggests. Measured by counting contexts per worker process over a full
  // run on a 10-core box: 5 workers (the default here) peaks at 26 of the 64
  // available, 4 peaks at 36, 6 peaks at 24. Scheduling is by duration, not by
  // count, so the busiest worker takes well over its even share — 36 at 4
  // workers, where an even split would be 27.
  //
  // Six, fixed rather than derived from the host: it holds the peak near 24
  // (~2.7x clear) for a project of ~110 tests — 113 as of this comment, and
  // the figures above were measured at 106 — costs nothing measurable here
  // (14.1s against 13.9s at 4), and gives the CI runner and the laptop the
  // same schedule, which a suite built on pixel comparison would rather have
  // than a second of wall time. The peak scales with the project's test
  // count, so revisit the number as it approaches ~250.
  //
  // Re-measured at Task 10 (Plan 5), because the suite had grown well past
  // what the paragraph above was written against. Worker indices are per
  // project — webkit takes 0-5 and chromium 6-11 — so contexts do not carry
  // across the two, and the number that matters is the peak within one
  // project. At 187 tests per project the peak was 39; at 212 it is 47. That
  // is roughly a third of a context per test added, so the ~63 ceiling is
  // about 260 tests per project away — call it fifty more tests. Still clear,
  // but by 1.4x rather than 2.7x: the "revisit near 250" note above is the
  // right instruction and it is now one plan off, not a distant one.
  //
  // PWDEBUG already forces a single worker inside Playwright, so this cannot
  // rescue a debug run of the whole suite — debug one test, not 106.
  workers: process.env.PWDEBUG ? 1 : 6,
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
