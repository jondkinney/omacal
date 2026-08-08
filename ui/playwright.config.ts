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
  // what the paragraph above was written against.
  //
  // Two things to count correctly, both of which a first pass at this got
  // wrong. Worker indices are per project — webkit takes 0-5 and chromium
  // 6-11 — so contexts never carry across the two, and the number that matters
  // is the peak inside one project, not across the run. And a *test* is not a
  // *context*: Playwright instantiates the `page` fixture lazily, so the
  // fifty-odd pure-function tests here (position, sanitize, location, and most
  // of eventform) open none at all. Counting tests rather than contexts
  // overstates the peak by about half.
  //
  // Measured at 225 tests per project, of which 175 create a context: the peak
  // is **31** of the ~63 available, in both engines. That is 2.0x clear. The
  // busiest worker is also not far off its even share — 31 against 29.2 — so
  // scheduling by duration costs a few contexts here, not a landslide.
  //
  // Contexts track context-creating tests, not the headline test count, so
  // read the bar that way: the ceiling is around 355 context-creating tests
  // per project, roughly 180 beyond where this sits. The "revisit near 250"
  // note above is still the right instruction; it is a bar on the count that
  // opens pages.
  //
  // PWDEBUG already forces a single worker inside Playwright, so this cannot
  // rescue a debug run of the whole suite — debug one test, not 106.
  workers: process.env.PWDEBUG ? 1 : 6,
  // Snapshots are the point of this suite; a stale one must fail, not silently update.
  updateSnapshots: 'missing',
  // Zero counted pixels, and a per-pixel threshold well below the default.
  //
  // Both halves, because they fail apart, and the half that looks like the
  // problem is not the one doing the work.
  //
  // `threshold` is pixelmatch's *per-pixel* gate: a pixel is not even a
  // candidate unless its squared-YIQ distance exceeds `35215 * threshold²`.
  // Unset, that default is 0.2 — a gate of **1408.6**, which is roughly a
  // 40-level step in every channel. Everything gentler was invisible at any
  // ratio whatsoever. That is how three header goldens spent two days showing
  // Year and Big Year as *disabled* after `188e3f8` made them live: measured
  // against the stale baselines, 7.02% of the frame differed and the gate
  // reduced it to 32 counted pixels (webkit) / 43 (chromium).
  //
  // But lowering the threshold alone would not have caught that either — at
  // 0.05 the count rises to 139/133, against the 294 that `maxDiffPixelRatio:
  // 0.01` allowed a 1280x23 frame. **The count limit is the load-bearing
  // half.** A ratio is a fraction of the *frame*, and a golden should not earn
  // a bigger budget for being mostly empty: at 0.01 the `weekgrid-*` pair
  // could move 9,216 pixels, and `rsvp-*` — before its frame was narrowed to
  // the block in the commit before this one — could have its entire subject
  // erased and come within 14 pixels of passing.
  //
  // 0.05 rather than 0 is measured, not cautious. Every change this repo has
  // actually produced, scored as squared-YIQ delta against the gates:
  //
  //     header drift (real)                 up to 1707   gate@0.05 =  88 -> caught
  //     AllDayBand squared corner (real)           309                  -> caught
  //     Chromium border-radius AA (legitimate)     ~2                   -> ignored
  //     a 1-level grey step (legitimate)           0.51                 -> ignored
  //
  // So 0.05 sits ~44x above every legitimate class measured and ~3.5x below
  // every real one. `threshold: 0` would have failed a layout-neutral commit
  // on two Chromium antialiasing pixels; 0.2 is blind to a squared chip corner.
  //
  // Playwright's own default is `maxDiffPixels: 0` when neither option is
  // given; this line is closer to that default than what it replaced.
  expect: { toHaveScreenshot: { maxDiffPixels: 0, threshold: 0.05 } },
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
