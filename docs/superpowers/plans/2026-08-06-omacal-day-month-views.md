# Day, Month and the View Switcher Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Day and Month views and a switcher between them and Week, so omacal covers the daily / weekly / monthly set it was asked for.

**Architecture:** `assemble_week` becomes a wrapper over a generalised `assemble_days(events, start_ms, n, tz)`, so Day is literally the week engine at `n = 1`. Month gets its own assembler because it needs no time positioning — six week-rows, each lane-packed at `row_len = 7` for spanning bars, plus a per-day list of timed events.

**Tech Stack:** Rust (`jiff`, sqlx/SQLite), Svelte 5 runes + TypeScript, Tauri v2, Playwright.

**Spec:** `docs/superpowers/specs/2026-08-06-omacal-views-design.md` — this plan is its §10 "Plan 3".
**Base:** `main` @ `f9b1bf1` — 245 Rust tests, 230 UI tests.

## Global Constraints

- **`selected` means displayed. `sync_enabled` means fetched.** No code may use one for the other's purpose.
- Time is `i64` epoch milliseconds. `chrono` stays confined to `crates/omacal-core`; `jiff` elsewhere.
- **Use `sqlx::query`/`query_as`/`query_scalar` (runtime-checked), never the `query!` macros** — they need `DATABASE_URL` at compile time and the build has none.
- **Never `{:?}`-log, print, or interpolate a `Tokens` value or any token string.**
- **The CSRF check in `sign_in` must not be removed or weakened.**
- **Demo mode must never write to the real database or reach Google.**
- **Never render event text with `{@html}`.**
- Svelte 5 runes only — `$props()`, `$state()`, `$derived()`, `$effect()`, `$bindable()`. No `export let`, no `$:`.
- **No live network calls in tests.**
- **Colour comes from the Omarchy theme** (`--accent`, `--bg`, `--hairline`, `--hour-rule`, `--muted`, `--surface`, `--text`, `--today-tint`). There is no semantic green or red, and adding a hardcoded one would be the first exception since Plan 1.
- **Never touch** `~/Library/Application Support/com.omacal.app/omacal.db` or `~/.config/omacal/config.toml`. If you copy the database, delete the copy **and its `-wal` and `-shm` sidecars**.
- `cargo test --workspace` starts at **245**. `npm --prefix ui run test:ui` starts at **230**.

## Verified fixtures

Computed, not reasoned about. Do not substitute your own.

| Name | Value | Is |
|---|---|---|
| `AUG_GRID_START` | `1785099600000` | **Mon** 2026-07-27 00:00 Europe/Sofia |
| `AUG_1` | `1785531600000` | **Sat** 2026-08-01 00:00 Sofia |
| `AUG_10_0900` | `1786341600000` | Mon 2026-08-10 09:00 Sofia |
| `AUG_31` | `1788123600000` | Mon 2026-08-31 00:00 Sofia |
| `SEP_7` | `1788728400000` | Mon 2026-09-07 00:00 Sofia — the grid's exclusive end |

**August 2026 is the fixture month on purpose.** It begins on a Saturday and runs Jul 27 → Sep 6, so a single month exercises leading out-of-month days, trailing ones, and the six-row case at once.

## What already exists and needs no change

- `signed_column(bounds: &[i64], ms: i64) -> i32` and `timed_column(bounds: &[i64], iv: &Interval) -> Option<usize>` both take a slice and are already length-agnostic.
- `pack_lanes(segs: &[Segment], row_len: u16, max_lanes: u8) -> (Vec<Lane>, Vec<usize>)` already takes `row_len`.
- `EventPopover` takes an anchor rect and `placePopover` is pure geometry — both views reuse them unchanged.

## File structure

| File | Responsibility |
|---|---|
| `src-tauri/src/commands.rs` | `day_boundaries(n)`, `assemble_days`, `assemble_month`, `MonthPayload` |
| `src-tauri/src/lib.rs` | `get_day`, `get_month` commands |
| `ui/src/lib/api.ts` | bindings and payload types |
| `ui/src/lib/MonthGrid.svelte` | the 6×7 grid |
| `ui/src/lib/ViewSwitcher.svelte` | the five-slot switcher |
| `ui/src/App.svelte` | current view, anchor date, keyboard |

---

### Task 1: Generalise the week engine

`assemble_week` is hardcoded to 7 in four places: `vec![Vec::new(); 7]`, `bounds[7]`, `pack_lanes(&segments, 7, 2)` and `(0..7)`. `day_boundaries` is hardcoded to 8 entries.

**Files:**
- Modify: `src-tauri/src/commands.rs:125` (`day_boundaries`), `:194` (`assemble_week`)

**Interfaces:**
- Produces:
  ```rust
  fn day_boundaries(start_ms: i64, n: usize, tz: &str) -> Vec<i64>;   // n + 1 entries
  pub fn assemble_days(events: &[StoredEvent], start_ms: i64, n: usize, tz: &str) -> WeekPayload;
  pub fn assemble_week(events: &[StoredEvent], week_start_ms: i64, tz: &str) -> WeekPayload;
  ```
  `assemble_week` keeps its exact signature — it becomes a one-line call to `assemble_days(events, week_start_ms, 7, tz)`. Every existing week test must therefore compile and pass **unchanged**; if any needs editing, stop and say so, because that means the wrapper is not equivalent.

- [ ] **Step 1: Write the failing test**

```rust
// src-tauri/src/commands.rs — tests module

#[test]
fn one_day_and_seven_days_agree_about_the_day_they_share() {
    // This is the guard that Day is genuinely the week engine at n=1 rather
    // than a parallel implementation that will drift. If someone later
    // "optimises" assemble_days for the n=1 case, this fails.
    let evs = vec![timed_event(AUG_10_0900, AUG_10_0900 + 30 * 60_000)];

    let week = assemble_days(&evs, AUG_GRID_START, 7, "Europe/Sofia");
    let day = assemble_days(&evs, week.days[0].start_ms, 1, "Europe/Sofia");

    assert_eq!(day.days.len(), 1);
    assert_eq!(day.days[0].start_ms, week.days[0].start_ms);
    assert_eq!(day.days[0].end_ms, week.days[0].end_ms);
    assert_eq!(
        day.days[0].events.len(),
        week.days[0].events.len(),
        "the same day assembled alone and as part of a week disagreed"
    );
}

#[test]
fn a_single_day_window_still_bounds_its_all_day_lane() {
    // pack_lanes is called with `n` as row_len; passing 7 for a one-day view
    // would let an all-day event claim columns that do not exist.
    let evs = vec![all_day_event(AUG_1, AUG_1 + 3 * 24 * 3_600_000)];
    let day = assemble_days(&evs, AUG_1, 1, "Europe/Sofia");
    for lane in &day.all_day {
        for seg in &lane.segments {
            assert!(seg.end_col < 1, "a 1-day view produced column {}", seg.end_col);
        }
    }
}
```

Use whatever event-building helpers the existing `commands.rs` tests use — read the tests module first and follow it. Do not invent `timed_event`/`all_day_event` if differently-named helpers are already there.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p omacal one_day_and_seven_days`
Expected: FAIL — `cannot find function assemble_days`.

- [ ] **Step 3: Generalise `day_boundaries`**

```rust
/// The `n + 1` local-midnight boundaries starting at `start_ms`.
///
/// `n + 1` and not `n`: every consumer needs the *end* of the last day, and a
/// DST day is not 24 hours, so it cannot be derived by addition.
fn day_boundaries(start_ms: i64, n: usize, tz: &str) -> Vec<i64> {
    use jiff::{Timestamp, ToSpan};

    let fallback = || (0..=n as i64).map(|i| start_ms + i * DAY_MS).collect::<Vec<_>>();

    let Ok(start) = Timestamp::from_millisecond(start_ms) else {
        return fallback();
    };
    let Ok(mut z) = start.in_tz(tz) else {
        return fallback();
    };

    let mut out = Vec::with_capacity(n + 1);
    out.push(z.timestamp().as_millisecond());
    for _ in 0..n {
        // Keep the existing body of this loop exactly as it is.
    }
    out
}
```

- [ ] **Step 4: Generalise `assemble_week`**

Rename it to `assemble_days`, add the `n: usize` parameter, and replace the four hardcoded sevens: `vec![Vec::new(); n]`, `bounds[n]`, `pack_lanes(&segments, n as u16, 2)`, `(0..n)`. Change nothing else — no reordering, no "while I'm here" cleanups. Then add the wrapper:

```rust
/// The week view. A thin wrapper so `assemble_days` has one caller shape and
/// Day view is provably the same engine — see
/// `one_day_and_seven_days_agree_about_the_day_they_share`.
pub fn assemble_week(events: &[StoredEvent], week_start_ms: i64, tz: &str) -> WeekPayload {
    assemble_days(events, week_start_ms, 7, tz)
}
```

- [ ] **Step 5: Verify**

Run: `cargo test --workspace`
Expected: PASS, 245 + 2. **No existing test may have been edited.** Confirm with `git diff` that the only changes inside the tests module are additions.

- [ ] **Step 6: Prove the tests guard**

Change `pack_lanes(&segments, n as u16, 2)` back to a literal `7` and confirm `a_single_day_window_still_bounds_its_all_day_lane` fails. Restore, `diff`. Then make `assemble_days` special-case `n == 1` by returning early with an empty `all_day` and confirm the agreement test fails. Restore, `diff`. **Assert each mutation applied before running** — a `replace` that matched nothing gives a green run meaning the opposite of what it looks like. Transcript in the report.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/commands.rs
git commit -m "refactor(commands): generalise the week engine to n days"
```

---

### Task 2: Day view

**Files:**
- Modify: `src-tauri/src/lib.rs` (add `get_day`, register it), `ui/src/lib/api.ts`, `ui/src/lib/WeekGrid.svelte`, `ui/tests/components.spec.ts`

**Interfaces:**
- Consumes: `assemble_days` from Task 1.
- Produces:
  ```rust
  // #[tauri::command] get_day(day_start_ms: i64) -> Result<WeekPayload, String>
  ```
  ```ts
  export const getDay: (dayStartMs: number) => Promise<WeekPayload>;
  ```
  `WeekGrid` gains `dayCount?: number` (default 7). One component, two views.

- [ ] **Step 1: Add the command**

Mirror `get_week` at `src-tauri/src/lib.rs:64-81` exactly, including its widening comment, with a two-day fetch either side:

```rust
#[tauri::command]
async fn get_day(
    state: tauri::State<'_, AppState>,
    day_start_ms: i64,
) -> Result<commands::WeekPayload, String> {
    let tz = display_tz(&state.pool);
    // Same widening as `get_week`, for the same reason: an event that begins
    // just before the day, or a DST-lengthened day, must not be missed.
    const DAY: i64 = 24 * 3_600_000;
    let events = omacal_store::events_in_window(
        &state.pool,
        day_start_ms - DAY,
        day_start_ms + 2 * DAY,
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(commands::assemble_days(&events, day_start_ms, 1, &tz))
}
```

Register it in `invoke_handler` alongside `get_week`. **A command that compiles but is not registered fails only at runtime, from the UI, with an unhelpful message.**

- [ ] **Step 2: Write the failing spec**

```ts
// ui/tests/components.spec.ts — in the WeekGrid describe block

test('a one-day grid renders a single column', async ({ page }) => {
  await page.goto('/tests/harness/index.html?c=WeekGrid&f=single-day');
  await expect(page.locator('.daycol')).toHaveCount(1);
});

test('overlapping events fan out fully in a one-day grid', async ({ page }) => {
  // Spec §4: Day always fans out rather than stacking into columns — there is
  // width to spare and no reason to compress.
  await page.goto('/tests/harness/index.html?c=WeekGrid&f=single-day-overlap');
  const blocks = page.locator('.daycol .ev');
  await expect(blocks).toHaveCount(2);
  const a = await blocks.nth(0).boundingBox();
  const b = await blocks.nth(1).boundingBox();
  expect(a!.x).not.toBe(b!.x);
  expect(Math.min(a!.width, b!.width)).toBeGreaterThan(80);
});
```

Add `single-day` and `single-day-overlap` fixtures with `dayCount: 1`. Use the day-column class the component actually renders — read `WeekGrid.svelte` and use its real class name rather than assuming `.daycol`.

- [ ] **Step 3: Run to verify failure**

Run: `npm --prefix ui run test:ui -- --project=chromium -g "one-day grid"`
Expected: FAIL — seven columns, not one.

- [ ] **Step 4: Implement**

`WeekGrid` takes `dayCount = 7` and renders `week.days` as it already does — the payload already carries the right number of columns, so most of this is removing an assumption of seven rather than adding a branch. Widen the column CSS to `repeat(var(--cols), 1fr)` driven by `week.days.length`.

**Also add `data-start-ms={day.start_ms}` to each day column.** Task 5's navigation specs assert on it — without a stable way to read which day is showing, "the anchor date survived the switch" cannot be tested at all, only eyeballed.

- [ ] **Step 5: Verify and prove the tests guard**

`npm --prefix ui run test:ui`, `npm --prefix ui run check`, `cargo test --workspace`. Then hardcode the grid back to seven columns and confirm the one-column test fails; restore, `diff`. Transcript in the report.

- [ ] **Step 6: Commit**

```bash
git add src-tauri ui
git commit -m "feat: day view"
```

---

### Task 3: The month assembler

**Files:**
- Modify: `src-tauri/src/commands.rs`

**Interfaces:**
- Produces:
  ```rust
  #[derive(serde::Serialize)]
  pub struct MonthCell {
      pub start_ms: i64,
      pub end_ms: i64,
      /// False for the leading and trailing days that belong to a neighbouring
      /// month. Drawn dimmed rather than blank so the grid stays rectangular.
      pub in_month: bool,
      /// Timed events for this day, sorted by start. The UI decides how many
      /// fit and renders `+N more` from what it drops — cell height is a
      /// layout question and the backend has no business guessing it.
      pub timed: Vec<UiEvent>,
  }
  #[derive(serde::Serialize)]
  pub struct MonthRow {
      pub cells: Vec<MonthCell>,          // always 7
      /// Multi-day and all-day events spanning this row, lane-packed at
      /// row_len 7. Indices point into `bar_events`.
      pub bars: Vec<Lane>,
      pub bar_events: Vec<UiEvent>,
      pub bar_overflow: Vec<usize>,
  }
  #[derive(serde::Serialize)]
  pub struct MonthPayload {
      pub rows: Vec<MonthRow>,            // always 6
      pub year: i32,
      pub month: u32,                     // 1-12
  }
  pub fn assemble_month(events: &[StoredEvent], year: i32, month: u32, tz: &str) -> MonthPayload;
  ```
  **Six rows always**, even when a month fits in five. A grid that changes height as you page through the year is worse than one dimmed row.

- [ ] **Step 1: Write the failing tests**

```rust
// src-tauri/src/commands.rs — tests module

#[test]
fn august_2026_starts_on_a_saturday_and_needs_six_rows() {
    // August 2026 begins Sat 1 Aug, so the grid runs Mon 27 Jul - Sun 6 Sep.
    // It exercises leading out-of-month days, trailing ones, and six rows.
    let m = assemble_month(&[], 2026, 8, "Europe/Sofia");
    assert_eq!(m.rows.len(), 6);
    assert_eq!(m.rows[0].cells.len(), 7);
    assert_eq!(m.rows[0].cells[0].start_ms, 1785099600000, "grid must start Mon 27 Jul");
    assert!(!m.rows[0].cells[0].in_month, "27 Jul belongs to July");
    assert!(m.rows[0].cells[5].in_month, "1 Aug is a Saturday, column 5");
    assert!(!m.rows[5].cells[6].in_month, "the last cell belongs to September");
}

#[test]
fn a_month_that_fits_in_five_rows_still_renders_six() {
    // Otherwise the grid changes height as you page through the year.
    let m = assemble_month(&[], 2026, 2, "Europe/Sofia");
    assert_eq!(m.rows.len(), 6);
}

#[test]
fn a_multi_day_event_crossing_a_row_boundary_appears_in_both_rows_clipped() {
    // Sun 2 Aug -> Tue 4 Aug straddles the first row's end. It must appear in
    // both rows, clipped to each, and never as one event counted twice.
    let evs = vec![all_day_event(1785963600000, 1786222800000)];
    let m = assemble_month(&evs, 2026, 8, "Europe/Sofia");
    let row0: usize = m.rows[0].bars.iter().map(|l| l.segments.len()).sum();
    let row1: usize = m.rows[1].bars.iter().map(|l| l.segments.len()).sum();
    assert_eq!(row0, 1, "row 0 should carry the Sunday tail");
    assert_eq!(row1, 1, "row 1 should carry the Mon-Tue head");
    for lane in &m.rows[0].bars {
        for s in &lane.segments {
            assert!(s.end_col <= 6, "a segment escaped its row: {}", s.end_col);
        }
    }
}

#[test]
fn timed_events_land_in_their_own_day_sorted() {
    let evs = vec![
        timed_event(1786341600000 + 3 * 3_600_000, 1786341600000 + 4 * 3_600_000),
        timed_event(1786341600000, 1786341600000 + 30 * 60_000),
    ];
    let m = assemble_month(&evs, 2026, 8, "Europe/Sofia");
    // Mon 10 Aug is row 2, column 0.
    let cell = &m.rows[2].cells[0];
    assert_eq!(cell.timed.len(), 2);
    assert!(cell.timed[0].start_ms < cell.timed[1].start_ms, "not sorted by start");
}
```

Verify the two epoch values in the third test against the fixture table before relying on them; if they do not land on Sun 2 Aug and Tue 4 Aug in Europe/Sofia, compute them and say so in your report rather than adjusting the assertions to match.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p omacal august_2026`
Expected: FAIL — `cannot find function assemble_month`.

- [ ] **Step 3: Implement**

Find the Monday on or before the 1st of the month, build `day_boundaries(grid_start, 42, tz)`, then for each of the six rows slice `bounds[r*7 ..= r*7+7]` and reuse the same loop shape `assemble_days` uses: skip `status == "cancelled"`, honour `suppressed_slots`, expand with `occurrences`, and route `is_all_day` events to `segments` via `signed_column` and timed ones to their cell via `timed_column`. Call `pack_lanes(&row_segments, 7, 3)` per row — three lanes, matching the spec's month rows.

Sort each cell's `timed` by `start_ms` before returning.

- [ ] **Step 4: Verify and prove the tests guard**

`cargo test --workspace`. Then: return five rows when the month fits, and confirm the five-row test fails. Then drop the per-row clipping so a segment can span rows, and confirm the boundary test fails. Restore each and verify with `diff`, asserting each mutation applied first. Transcript in the report.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands.rs
git commit -m "feat(commands): month assembly with spanning bars"
```

---

### Task 4: Month view

**Files:**
- Create: `ui/src/lib/MonthGrid.svelte`
- Modify: `src-tauri/src/lib.rs` (`get_month`), `ui/src/lib/api.ts`, `ui/tests/fixtures.ts`, `ui/tests/harness/mount.svelte.ts`, `ui/tests/components.spec.ts`

**Interfaces:**
- Consumes: `assemble_month` from Task 3.
- Produces:
  ```rust
  // #[tauri::command] get_month(year: i32, month: u32) -> Result<commands::MonthPayload, String>
  ```
  ```ts
  export const getMonth: (year: number, month: number) => Promise<MonthPayload>;
  ```
  `MonthGrid` takes `{ month, onopen, ondaypick }` where `onopen(event, rect)` matches `WeekGrid`'s existing popover contract and `ondaypick(startMs)` asks the parent to switch to Day view.

- [ ] **Step 1: Write the failing specs**

```ts
// ui/tests/components.spec.ts

test.describe('MonthGrid', () => {
  const show = (f: string) => `/tests/harness/index.html?c=MonthGrid&f=${f}`;

  test('renders six rows of seven, with out-of-month days dimmed', async ({ page }) => {
    await page.goto(show('august'));
    await expect(page.locator('.mrow')).toHaveCount(6);
    await expect(page.locator('.mcell')).toHaveCount(42);
    await expect(page.locator('.mcell.out')).toHaveCount(11); // 5 leading + 6 trailing
  });

  test('a multi-day event is one bar, not one chip per day', async ({ page }) => {
    await page.goto(show('august'));
    await expect(page.locator('.bar', { hasText: 'Berlin trip' })).toHaveCount(1);
  });

  test('a timed event shows a dot and a title, and no time', async ({ page }) => {
    // Spec §2: a time prefix costs about a third of a narrow cell.
    await page.goto(show('august'));
    const line = page.locator('.mcell .timed').first();
    await expect(line).toContainText('Standup');
    await expect(line).not.toContainText(':');
  });

  test('+N more asks the parent for that day', async ({ page }) => {
    await page.goto(show('busy-day'));
    await page.locator('.more').first().click();
    const picked = await page.evaluate(() => (window as any).__lastDayPick);
    expect(picked).toBe(1786341600000); // Mon 10 Aug
  });

  test('clicking the day number asks the parent for that day too', async ({ page }) => {
    await page.goto(show('august'));
    await page.locator('.mcell .num').nth(14).click();
    expect(await page.evaluate(() => (window as any).__lastDayPick)).toBeTruthy();
  });

  test('clicking an event opens the popover, not the day', async ({ page }) => {
    await page.goto(show('august'));
    await page.locator('.mcell .timed').first().click();
    expect(await page.evaluate(() => (window as any).__lastOpen)).toBeTruthy();
    expect(await page.evaluate(() => (window as any).__lastDayPick)).toBeFalsy();
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `npm --prefix ui run test:ui -- --project=chromium -g "six rows of seven"`
Expected: FAIL — no such component.

- [ ] **Step 3: Implement**

`MonthGrid.svelte`: six `.mrow`s, each with its `.bar`s positioned from `Lane.segments`' `start_col`/`end_col`, then seven `.mcell`s. A cell shows `.num`, then up to `MAX_LINES = 3` `.timed` lines as a coloured dot plus title, then `.more` reading `+N more` when `timed.length > MAX_LINES`.

An event line calls `onopen(event, rect)` and **must stop propagation** so the cell's day-pick handler does not also fire — that is what the last spec above pins.

**The specs above read `window.__lastDayPick` and `window.__lastOpen`, which do not exist yet.** The harness already captures calls this way — `__lastRespondCall` in `ui/tests/harness/tauri.ts` is the precedent — so follow that pattern and add the two capture globals in the `MonthGrid` mount branch of `ui/tests/harness/mount.svelte.ts`. Do not invent a different mechanism.

Out-of-month cells get `.out` and are dimmed via `--muted`, not hidden.

- [ ] **Step 4: Add the command and binding**

`get_month(year, month)` fetches `events_in_window` across the whole 42-day grid, widened a day either side, and calls `assemble_month`. Register it in `invoke_handler`.

- [ ] **Step 5: Verify and prove the tests guard**

`npm --prefix ui run check`, `npm --prefix ui run test:ui`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`. Then: remove the `stopPropagation` and confirm the last spec fails; render out-of-month cells as blank and confirm the dimming spec fails. Restore each with `diff`, asserting each applied. Transcript in the report.

- [ ] **Step 6: Commit**

```bash
git add src-tauri ui
git commit -m "feat(ui): month view"
```

---

### Task 5: The switcher, the keyboard, and the anchor date

**Files:**
- Create: `ui/src/lib/ViewSwitcher.svelte`
- Modify: `ui/src/App.svelte`, `ui/src/lib/Header.svelte`, `ui/tests/app.spec.ts`

**Interfaces:**
- Consumes: `getDay`, `getMonth`, `getWeek`.
- Produces:
  ```ts
  export type View = 'day' | 'week' | 'month' | 'year' | 'bigyear';
  ```
  `ViewSwitcher` takes `{ view, onpick }` and renders **five** slots, with `year` and `bigyear` disabled — Plan 4 fills them in rather than rebuilding the control.

- [ ] **Step 1: Write the failing specs**

```ts
// ui/tests/app.spec.ts

test('the switcher offers five views, two of them not yet built', async ({ page }) => {
  await page.goto(app('connected'));
  await expect(page.locator('.vswitch button')).toHaveCount(5);
  await expect(page.getByRole('button', { name: 'Year' })).toBeDisabled();
  await expect(page.getByRole('button', { name: 'Big Year' })).toBeDisabled();
});

test('number keys switch views', async ({ page }) => {
  await page.goto(app('connected'));
  await page.keyboard.press('3');
  await expect(page.locator('.mrow')).toHaveCount(6);
  await page.keyboard.press('1');
  await expect(page.locator('.daycol')).toHaveCount(1);
});

test('the anchor date survives a view switch', async ({ page }) => {
  // Spec §5: switching Month -> Day lands on the day you were looking at, not
  // on today. This is what makes "+N more" and the day-number click work as
  // handoffs rather than jumps.
  await page.goto(app('connected'));
  await page.keyboard.press('3');
  await page.locator('.mcell .num').nth(14).click();
  await expect(page.locator('.daycol')).toHaveCount(1);
  const shown = await page.locator('.daycol').getAttribute('data-start-ms');
  expect(Number(shown)).toBe(1786341600000); // the day that was clicked
});

test('H and L step by the current view\'s unit', async ({ page }) => {
  await page.goto(app('connected'));
  await page.keyboard.press('2');
  const before = await page.locator('.daycol').first().getAttribute('data-start-ms');
  await page.keyboard.press('l');
  const after = await page.locator('.daycol').first().getAttribute('data-start-ms');
  expect(Number(after) - Number(before)).toBe(7 * 24 * 3600 * 1000);
});

test('T returns to today', async ({ page }) => {
  await page.goto(app('connected'));
  const col = page.locator('.daycol').first();
  // Capture where the app opened rather than inventing a global to remember
  // it — whatever "today" is for the fixture clock, two steps forward and T
  // must land back on exactly this value.
  const opened = await col.getAttribute('data-start-ms');
  await page.keyboard.press('l');
  await page.keyboard.press('l');
  expect(await col.getAttribute('data-start-ms')).not.toBe(opened);
  await page.keyboard.press('t');
  expect(await col.getAttribute('data-start-ms')).toBe(opened);
});
```

`.daycol` needs a `data-start-ms` attribute for these to be assertable; add it in Task 2 if you are reading this plan out of order.

- [ ] **Step 2: Run to verify failure**

Run: `npm --prefix ui run test:ui -- --project=chromium -g "five views"`
Expected: FAIL — no switcher.

- [ ] **Step 3: Implement**

`App.svelte` holds `view: View` and `anchorMs: number` in `$state`. A `$derived` picks which loader to call. Keys are handled with `<svelte:window onkeydown>` — **ignore the event when the target is an input, a textarea, or inside the event popover**, or typing in a future search box will teleport the user to March.

`H`/`L` step by one day, one week, or one calendar month depending on `view`. `T` sets `anchorMs` back to today's local midnight. Month's `ondaypick` sets `anchorMs` and switches to `day`.

- [ ] **Step 4: Verify and prove the tests guard**

Full suites plus `check` and clippy. Then: make `ondaypick` set the view without setting `anchorMs` and confirm the anchor-survival spec fails — that is the trap this task exists to avoid. Restore, `diff`, asserting the mutation applied. Transcript in the report.

- [ ] **Step 5: Commit**

```bash
git add ui
git commit -m "feat(ui): view switcher, number keys, and a shared anchor date"
```

---

## Definition of Done

- [ ] Day, Week and Month all render, and the switcher moves between them
- [ ] `1`/`2`/`3` switch views; `4` and `5` are visibly present but disabled
- [ ] `H`/`L` step by the current view's unit; `T` returns to today
- [ ] The anchor date survives every switch — Month → Day lands on the day you clicked
- [ ] Month shows six rows always, with out-of-month days dimmed rather than blank
- [ ] A multi-day event is one spanning bar per row it touches, never one chip per day
- [ ] A month cell shows a dot and a title, no time, and `+N more` opens that day
- [ ] Clicking an event in any view opens the popover, correctly placed
- [ ] `cargo test --workspace` ≥ 255, `npm --prefix ui run test:ui` ≥ 250, clippy and `check` clean

## Deliberately not in this plan

- Year and Big Year — Plan 4, per spec §10. The switcher ships their slots disabled.
- Unsynced-range shading (spec §6). Day, Week and Month all sit well inside the
  `now − 180d … now + 365d` window in normal use; it becomes reachable, and
  necessary, once a view can address a whole year.
- The filmstrip toggle, search, and full keyboard navigation beyond the above.
- Creating, editing or deleting events.
