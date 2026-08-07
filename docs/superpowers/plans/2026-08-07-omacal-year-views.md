# Year and Big Year Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the 12-up Year grid and the Big Year ribbon, and light up the two switcher slots Plan 3 left disabled.

**Architecture:** Two new assemblers alongside `assemble_days`/`assemble_month`. Year is mostly date arithmetic — per-day "does this have an all-day event" flags. Big Year is fourteen 28-day rows, each lane-packed by the same `pack_lanes` the week's all-day band uses, at `row_len = 28`. Both payloads carry the synced window so the views can say "not fetched" rather than draw an empty January as a free one.

**Tech Stack:** Rust (`jiff`, sqlx/SQLite), Svelte 5 runes + TypeScript, Tauri v2, Playwright.

**Spec:** `docs/superpowers/specs/2026-08-06-omacal-views-design.md` — §4, §6, and its §10 "Plan 4".
**Base:** `main` @ `10c53b7` — 255 Rust tests, 276 UI tests.

## Global Constraints

- **`selected` means displayed. `sync_enabled` means fetched.** No code may use one for the other's purpose.
- Time is `i64` epoch milliseconds. `chrono` stays confined to `crates/omacal-core`; `jiff` elsewhere.
- **Never `{:?}`-log, print, or interpolate a `Tokens` value.** `Tokens` has a hand-written redacting `Debug`.
- **The CSRF check in `sign_in` must not be removed or weakened.**
- **Use `sqlx::query`/`query_as`/`query_scalar`, never the `query!` macros.**
- **Demo mode must never write to the real database or reach Google.**
- **Never render event text with `{@html}`.**
- Svelte 5 runes only — no `export let`, no `$:`.
- **Colour comes from the Omarchy theme** (`--accent`, `--bg`, `--hairline`, `--hour-rule`, `--muted`, `--surface`, `--text`, `--today-tint`); per-event colour from the payload. There is no semantic green or red.
- **No live network calls in tests.**
- **Never touch** `~/Library/Application Support/com.omacal.app/omacal.db` or `~/.config/omacal/config.toml`. If you copy the database, delete the copy **and its `-wal` and `-shm` sidecars**.
- `cargo test --workspace` starts at **255**. `npm --prefix ui run test:ui` starts at **276**.

## Verified fixtures

Computed, not reasoned about. Do not substitute your own.

| Name | Value | Is |
|---|---|---|
| `RIBBON_START` | `1766959200000` | **Mon 29 Dec 2025** 00:00 Europe/Sofia — the Monday on or before 1 Jan 2026 |
| `ROW1_START` | `1769378400000` | Mon 26 Jan 2026 — row 1 |
| `ROW13_START` | `1798408800000` | Mon 28 Dec 2026 — row 13, the last |
| weekend columns | `[5,6,12,13,19,20,26,27]` | the same in every row, which is the whole point |

1 Jan 2026 is a **Thursday**. 14 rows × 28 days = 392, running Mon 29 Dec 2025 → Sun 24 Jan 2027, which covers 2026 with overhang at both ends.

## What already exists and needs no change

- **`pack_lanes(segs, row_len: u16, max_lanes: u8)`** already takes `row_len`. Big Year passes 28.
- **`Lane` already carries `cont_left` and `cont_right`** (`crates/omacal-core/src/lanes.rs:13-25`), set when a segment was clipped at a row edge. The spec's `‹` continuation marker is a render of a flag that already exists — do not recompute it.
- **`n_day_boundaries(start_ms, n, tz)`** returns `n + 1` DST-aware boundaries at any `n`.
- `EventPopover` takes an anchor rect; `placePopover` is pure geometry.

## Three carry-forwards from Plan 3 — all land in this plan

1. **`step()` must gain a `year` branch in the same commit that enables the switcher slots.** Today `step()` returns early for `year`/`bigyear` while the arrows would announce "Previous year" — a control naming motion it does not perform. Task 5 owns both halves; splitting them ships dead arrows.
2. **The `#[cfg(test)]` two-arg `day_boundaries` shim** (`src-tauri/src/commands.rs:193-196`) was kept because four tests call it with two arguments. Big Year needs boundaries at `n = 392`, which is the fifth caller at a different `n` that makes the shim dead weight. **Task 3 deletes it** and edits those four call sites for arity — mechanical, and free when done by the plan that needs it.
3. **`signed_column`'s overflow magnitude is unverified by any test** — `pack_lanes` reads only the sign and clips the magnitude, so nothing observes it today. Its doc comment carries a tripwire saying so. **If Big Year's continuation markers end up reading that magnitude, the tripwire has fired and you must add the test.** If they only read `cont_left`/`cont_right`, leave it.

## File structure

| File | Responsibility |
|---|---|
| `src-tauri/src/commands.rs` | `assemble_year`, `assemble_big_year`, their payloads |
| `src-tauri/src/lib.rs` | `get_year`, `get_big_year`, the shared synced-window helper |
| `ui/src/lib/api.ts` | bindings and payload types |
| `ui/src/lib/YearGrid.svelte` | the 12-up grid |
| `ui/src/lib/BigYearRibbon.svelte` | the 14×28 ribbon and its legend |
| `ui/src/App.svelte` | the two new views, `step()`'s year branch |
| `ui/src/lib/ViewSwitcher.svelte` | the two slots stop being disabled |

---

### Task 1: The synced window, shared

Both new views can address dates the app never fetched. §6 requires those to render as **unsynced, not empty** — an empty January drawn identically to a free January is a false statement.

The window is computed today at `src-tauri/src/lib.rs:459` inside the sync loop: `(now - 180 * DAY, now + 365 * DAY)`. The commands need the same two numbers, and two definitions would drift.

**Files:**
- Modify: `src-tauri/src/lib.rs:459`

**Interfaces:**
- Produces:
  ```rust
  /// The window the app keeps synced, as (from_ms, to_ms).
  pub(crate) fn synced_window(now_ms: i64) -> (i64, i64);
  ```
  Takes `now_ms` rather than reading the clock, so it is testable.

- [ ] **Step 1: Write the failing test**

```rust
// src-tauri/src/lib.rs — tests module

#[test]
fn the_synced_window_is_180_days_back_and_365_forward() {
    // Both year views render dates outside this, and must say "not fetched"
    // rather than draw them as free. One definition, so the views and the
    // sync loop can never disagree about where the edge is.
    const DAY: i64 = 24 * 3_600_000;
    let now = 1_786_341_600_000; // Mon 10 Aug 2026 09:00 Europe/Sofia
    let (from, to) = synced_window(now);
    assert_eq!(from, now - 180 * DAY);
    assert_eq!(to, now + 365 * DAY);
    assert!(from < now && now < to);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p omacal synced_window`
Expected: FAIL — `cannot find function synced_window`.

- [ ] **Step 3: Extract it**

```rust
/// The window the app keeps synced.
///
/// Extracted so the year views and the sync loop cannot disagree about where
/// the edge is: both render decisions ("is this date fetched?") and fetch
/// decisions ("what should I ask Google for?") must read one definition.
pub(crate) fn synced_window(now_ms: i64) -> (i64, i64) {
    const DAY: i64 = 24 * 3_600_000;
    (now_ms - 180 * DAY, now_ms + 365 * DAY)
}
```

Then change the sync loop's line to call it. **Its behaviour must not change** — same two numbers from the same `now`.

- [ ] **Step 4: Verify and prove the test guards**

`cargo test --workspace`. Then change `180` to `181` and confirm the test fails; restore, verify with `diff`. **Assert the mutation applied before running** — a `replace` that matched nothing gives a green run meaning the opposite of what it looks like.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "refactor(app): one definition of the synced window"
```

---

### Task 2: The Year assembler and view

The 12-up grid, for date arithmetic rather than planning: "what weekday is the 14th". A day with an all-day event gets a dot; today is a filled disc; clicking any date switches to Day view.

**Files:**
- Modify: `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`, `ui/src/lib/api.ts`
- Create: `ui/src/lib/YearGrid.svelte`
- Modify: `ui/tests/fixtures.ts`, `ui/tests/harness/mount.svelte.ts`, `ui/tests/components.spec.ts`

**Interfaces:**
- Consumes: `synced_window` from Task 1, `n_day_boundaries`, `occurrences`, `suppressed_slots`.
- Produces:
  ```rust
  #[derive(serde::Serialize)]
  pub struct YearDay {
      pub start_ms: i64,
      pub day: u32,          // 1-31
      /// At least one all-day event. Timed events do not dot the year grid —
      /// this view answers "what is blocked out", not "how busy am I".
      pub has_all_day: bool,
      /// Outside the synced window. Drawn distinctly from an in-window day
      /// with nothing on it (§6).
      pub unsynced: bool,
  }
  #[derive(serde::Serialize)]
  pub struct YearMonth {
      pub month: u32,        // 1-12
      /// Leading blanks before the 1st, so the weekday columns line up.
      pub lead_blanks: usize,
      pub days: Vec<YearDay>,
  }
  #[derive(serde::Serialize)]
  pub struct YearPayload { pub year: i32, pub months: Vec<YearMonth> }
  pub fn assemble_year(events: &[StoredEvent], year: i32, now_ms: i64, tz: &str) -> YearPayload;
  // #[tauri::command] get_year(year: i32) -> Result<YearPayload, String>
  ```
  ```ts
  export const getYear: (year: number) => Promise<YearPayload>;
  ```

- [ ] **Step 1: Write the failing Rust tests**

```rust
// src-tauri/src/commands.rs — tests module

#[test]
fn a_year_has_twelve_months_with_the_right_day_counts() {
    let y = assemble_year(&[], 2026, 1_786_341_600_000, "Europe/Sofia");
    assert_eq!(y.months.len(), 12);
    assert_eq!(y.months[0].days.len(), 31, "January");
    assert_eq!(y.months[1].days.len(), 28, "February 2026 is not a leap year");
    assert_eq!(y.months[10].days.len(), 30, "November");
}

#[test]
fn a_leap_february_has_twenty_nine_days() {
    let y = assemble_year(&[], 2028, 1_786_341_600_000, "Europe/Sofia");
    assert_eq!(y.months[1].days.len(), 29);
}

#[test]
fn lead_blanks_line_the_first_up_under_its_weekday() {
    // 1 Jan 2026 is a Thursday, so Monday-first means three blanks.
    let y = assemble_year(&[], 2026, 1_786_341_600_000, "Europe/Sofia");
    assert_eq!(y.months[0].lead_blanks, 3);
    // 1 Jun 2026 is a Monday — no blanks at all.
    assert_eq!(y.months[5].lead_blanks, 0);
}

#[test]
fn only_all_day_events_dot_the_year_grid() {
    // A timed meeting is not "blocked out"; this view answers what is.
    let timed = vec![timed_event(1_786_341_600_000, 1_786_341_600_000 + 3_600_000)];
    let y = assemble_year(&timed, 2026, 1_786_341_600_000, "Europe/Sofia");
    assert!(y.months.iter().all(|m| m.days.iter().all(|d| !d.has_all_day)));
}

#[test]
fn days_outside_the_synced_window_are_marked_unsynced() {
    // From Aug 2026 the window starts in Feb, so January of the *current*
    // year is already outside it — an empty January must not read as free.
    let y = assemble_year(&[], 2026, 1_786_341_600_000, "Europe/Sofia");
    assert!(y.months[0].days[0].unsynced, "1 Jan 2026 is before now-180d");
    assert!(!y.months[7].days[0].unsynced, "1 Aug 2026 is inside the window");
}
```

Use whatever event-building helpers the existing `commands.rs` tests use — read the tests module first rather than assuming `timed_event` exists under that name.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p omacal assemble_year`
Expected: FAIL — `cannot find function assemble_year`.

- [ ] **Step 3: Implement the assembler**

For each month, find its local midnight boundaries with `n_day_boundaries(month_start, days_in_month, tz)`, mark `has_all_day` from any `is_all_day` occurrence landing that day (skipping `status == "cancelled"` and honouring `suppressed_slots`, exactly as `assemble_month` does), and set `unsynced` from `synced_window(now_ms)`. `lead_blanks` is the weekday index of the 1st with Monday as 0.

- [ ] **Step 4: Add the command**

`get_year(year)` fetches `events_in_window` across the whole year widened a day either side, passes `now_ms()` through, and calls `assemble_year`. **Register it in `invoke_handler`** — a command that compiles but is not registered fails only at runtime, from the UI, with an unhelpful message. This project has shipped that mistake.

- [ ] **Step 5: Write the failing UI specs**

```ts
// ui/tests/components.spec.ts

test.describe('YearGrid', () => {
  const show = (f: string) => `/tests/harness/index.html?c=YearGrid&f=${f}`;

  test('renders twelve months', async ({ page }) => {
    await page.goto(show('y2026'));
    await expect(page.locator('.ymonth')).toHaveCount(12);
  });

  test('a day with an all-day event gets a dot', async ({ page }) => {
    await page.goto(show('y2026'));
    await expect(page.locator('.yday.dotted')).toHaveCount(1);
  });

  test('today is a filled disc', async ({ page }) => {
    await page.goto(show('y2026'));
    await expect(page.locator('.yday.today')).toHaveCount(1);
  });

  test('unsynced days are distinct from empty ones', async ({ page }) => {
    // §6: an empty January must not read as a free January.
    await page.goto(show('y2026'));
    const unsynced = page.locator('.yday.unsynced').first();
    await expect(unsynced).toBeVisible();
    await expect(unsynced).not.toHaveClass(/dotted/);
  });

  test('clicking a date asks the parent for that day', async ({ page }) => {
    await page.goto(show('y2026'));
    await page.locator('.yday').nth(200).click();
    expect(await page.evaluate(() => (window as any).__lastDayPick)).toBeTruthy();
  });
});
```

`__lastDayPick` follows the pattern `MonthGrid`'s fixtures already established in `ui/tests/harness/mount.svelte.ts` — reuse it rather than inventing a second mechanism.

- [ ] **Step 6: Implement `YearGrid.svelte`**

Twelve `.ymonth` blocks, each with a weekday header row and `lead_blanks` empty cells before day 1. A `.yday` carries `.dotted` when `has_all_day`, `.today` when it is today, `.unsynced` when `unsynced`. Clicking one calls `ondaypick(start_ms)`.

`.unsynced` must be visually distinct from a plain empty day — a hatch or reduced opacity on the *cell*, not merely the absence of a dot, since absence is exactly what it must not be confused with.

- [ ] **Step 7: Verify and prove the tests guard**

`cargo test --workspace`, `npm --prefix ui run test:ui`, `npm --prefix ui run check`, `cargo clippy --workspace --all-targets -- -D warnings`.

Then: make `has_all_day` true for timed events too and confirm `only_all_day_events_dot_the_year_grid` fails. Make `unsynced` always false and confirm both the Rust and the UI unsynced tests fail. Restore each, verify with `diff`, **asserting each mutation applied first**. Transcript in the report.

- [ ] **Step 8: Commit**

```bash
git add src-tauri ui
git commit -m "feat: the 12-up year view"
```

---

### Task 3: The Big Year assembler

Fourteen rows of 28 days, all-day and multi-day events only, lane-packed per row.

**Files:**
- Modify: `src-tauri/src/commands.rs`

**Interfaces:**
- Consumes: `pack_lanes`, `n_day_boundaries`, `synced_window`.
- Produces:
  ```rust
  #[derive(serde::Serialize)]
  pub struct RibbonDay { pub start_ms: i64, pub in_year: bool, pub unsynced: bool }
  #[derive(serde::Serialize)]
  pub struct RibbonRow {
      pub days: Vec<RibbonDay>,      // always 28
      pub pills: Vec<Lane>,          // packed at row_len 28, 3 lanes
      pub pill_events: Vec<UiEvent>, // `Lane.idx` indexes into this
      pub overflow: Vec<usize>,
  }
  #[derive(serde::Serialize)]
  pub struct BigYearPayload {
      pub year: i32,
      pub rows: Vec<RibbonRow>,      // always 14
      /// Every calendar with at least one pill, for the legend.
      pub legend: Vec<LegendEntry>,
  }
  #[derive(serde::Serialize)]
  pub struct LegendEntry { pub name: String, pub color: Option<String> }
  pub fn assemble_big_year(events: &[StoredEvent], year: i32, now_ms: i64, tz: &str) -> BigYearPayload;
  ```

**Also in this task: delete the `#[cfg(test)]` `day_boundaries` shim** at `src-tauri/src/commands.rs:193-196` and change its four callers to `n_day_boundaries(x, 7, tz)`. This task is the fifth caller at a different `n`, which is exactly the condition Plan 3 said would make the shim dead weight.

- [ ] **Step 1: Write the failing tests**

```rust
// src-tauri/src/commands.rs — tests module

#[test]
fn the_ribbon_starts_on_the_monday_before_new_year_and_runs_fourteen_rows() {
    let b = assemble_big_year(&[], 2026, 1_786_341_600_000, "Europe/Sofia");
    assert_eq!(b.rows.len(), 14);
    assert_eq!(b.rows[0].days.len(), 28);
    // 1 Jan 2026 is a Thursday, so the ribbon opens Mon 29 Dec 2025.
    assert_eq!(b.rows[0].days[0].start_ms, 1766959200000);
    assert_eq!(b.rows[1].days[0].start_ms, 1769378400000);
    assert_eq!(b.rows[13].days[0].start_ms, 1798408800000);
    assert!(!b.rows[0].days[0].in_year, "29 Dec 2025 belongs to the year before");
}

#[test]
fn every_row_puts_its_weekends_in_the_same_columns() {
    // This is the entire reason rows are 28 days and not the reference
    // image's 29: at 28 the weekend columns are constant, so the shading
    // reads as straight vertical stripes instead of drifting diagonally.
    // A later "tidy-up" to 29 would break exactly this.
    use jiff::{civil::Weekday, Timestamp};
    let b = assemble_big_year(&[], 2026, 1_786_341_600_000, "Europe/Sofia");
    let expected = [5usize, 6, 12, 13, 19, 20, 26, 27];
    for (r, row) in b.rows.iter().enumerate() {
        let weekend: Vec<usize> = row
            .days
            .iter()
            .enumerate()
            .filter(|(_, d)| {
                let wd = Timestamp::from_millisecond(d.start_ms)
                    .unwrap()
                    .in_tz("Europe/Sofia")
                    .unwrap()
                    .weekday();
                wd == Weekday::Saturday || wd == Weekday::Sunday
            })
            .map(|(i, _)| i)
            .collect();
        assert_eq!(weekend, expected, "row {r} weekend columns drifted");
    }
}

#[test]
fn a_span_crossing_a_row_boundary_splits_and_both_halves_know_it() {
    // 28-day rows guarantee this happens; `pack_lanes` already sets
    // `cont_left`/`cont_right` when it clips, so the renderer's `‹` marker
    // is a flag being read, not recomputed.
    // Sun 25 Jan .. Tue 27 Jan 2026 inclusive, so the end is Wed 28 at 00:00 —
    // Google's all-day end is exclusive. Row 0 ends on Sun 25 Jan, so this
    // straddles the row 0/1 boundary by construction.
    let ev = vec![all_day_event(1769292000000, 1769551200000)];
    let b = assemble_big_year(&ev, 2026, 1_786_341_600_000, "Europe/Sofia");
    let r0: Vec<_> = b.rows[0].pills.iter().collect();
    let r1: Vec<_> = b.rows[1].pills.iter().collect();
    assert_eq!(r0.len(), 1, "row 0 carries the Sunday tail");
    assert_eq!(r1.len(), 1, "row 1 carries the Mon-Tue head");
    assert!(r0[0].cont_right, "row 0's half continues past the row");
    assert!(r1[0].cont_left, "row 1's half began before the row");
}

#[test]
fn only_all_day_and_multi_day_events_reach_the_ribbon() {
    let timed = vec![timed_event(1_786_341_600_000, 1_786_341_600_000 + 3_600_000)];
    let b = assemble_big_year(&timed, 2026, 1_786_341_600_000, "Europe/Sofia");
    assert!(b.rows.iter().all(|r| r.pills.is_empty()));
}
```

**Verify the two epoch values in the third test against the fixture table before relying on them.** If they do not land on Sun 25 Jan and Wed 28 Jan 2026 in Europe/Sofia (exclusive end), compute the right ones and say so in your report rather than adjusting the assertions to fit whatever the code produces.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p omacal assemble_big_year`
Expected: FAIL — `cannot find function assemble_big_year`.

- [ ] **Step 3: Implement**

Find the Monday on or before 1 January, build `n_day_boundaries(ribbon_start, 392, tz)`, then per row slice `bounds[r*28 ..= r*28+28]` and run `pack_lanes(&row_segments, 28, 3)`. Skip `status == "cancelled"`, honour `suppressed_slots`, and take only `is_all_day` events. Build `legend` from the distinct calendars of the events that produced pills.

- [ ] **Step 4: Delete the shim**

Remove `#[cfg(test)] fn day_boundaries` at `:193-196` and change its four callers to `n_day_boundaries(x, 7, tz)`. **This is arity only — no assertion may change.** If any test's expectations need editing, stop and say so.

- [ ] **Step 5: Verify and prove the tests guard**

`cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`.

Then: change the row length to 29 and confirm `every_row_puts_its_weekends_in_the_same_columns` fails — that test exists precisely to stop a later reader "fixing" 28 to 29. Then stop taking `cont_left`/`cont_right` from `pack_lanes` and hardcode them false, and confirm the split test fails. Restore each, verify with `diff`, **asserting each mutation applied first**.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/commands.rs
git commit -m "feat(commands): big year ribbon assembly"
```

---

### Task 4: The Big Year ribbon view

**Files:**
- Create: `ui/src/lib/BigYearRibbon.svelte`
- Modify: `src-tauri/src/lib.rs` (`get_big_year`), `ui/src/lib/api.ts`, `ui/tests/fixtures.ts`, `ui/tests/harness/mount.svelte.ts`, `ui/tests/components.spec.ts`

**Interfaces:**
- Consumes: `assemble_big_year` from Task 3.
- Produces:
  ```rust
  // #[tauri::command] get_big_year(year: i32) -> Result<commands::BigYearPayload, String>
  ```
  ```ts
  export const getBigYear: (year: number) => Promise<BigYearPayload>;
  ```
  `BigYearRibbon` takes `{ ribbon, onopen }`, where `onopen(event, rect)` matches the popover contract `WeekGrid` and `MonthGrid` already use.

- [ ] **Step 1: Write the failing specs**

```ts
// ui/tests/components.spec.ts

test.describe('BigYearRibbon', () => {
  const show = (f: string) => `/tests/harness/index.html?c=BigYearRibbon&f=${f}`;

  test('renders fourteen rows of twenty-eight', async ({ page }) => {
    await page.goto(show('y2026'));
    await expect(page.locator('.rrow')).toHaveCount(14);
    await expect(page.locator('.rrow').first().locator('.rday')).toHaveCount(28);
  });

  test('weekend shading forms straight vertical stripes', async ({ page }) => {
    // The 28-day row exists for this. Assert the column indices, not a
    // screenshot: this is the property, and a screenshot would also pass
    // for a subtly different one.
    await page.goto(show('y2026'));
    for (const r of [0, 7, 13]) {
      const cols: number[] = [];
      const days = page.locator('.rrow').nth(r).locator('.rday');
      for (let i = 0; i < 28; i++) {
        if ((await days.nth(i).getAttribute('class'))?.includes('wknd')) cols.push(i);
      }
      expect(cols).toEqual([5, 6, 12, 13, 19, 20, 26, 27]);
    }
  });

  test('days outside the year are dimmed, not blank', async ({ page }) => {
    await page.goto(show('y2026'));
    const out = page.locator('.rday.out').first();
    await expect(out).toBeVisible();
    await expect(out).not.toBeEmpty();
  });

  test('a span crossing a row shows a continuation marker on both halves', async ({ page }) => {
    await page.goto(show('crossing'));
    await expect(page.locator('.pill.cont')).toHaveCount(2);
  });

  test('the legend names each calendar that has a pill', async ({ page }) => {
    await page.goto(show('y2026'));
    await expect(page.locator('.legend .item')).toHaveCount(2);
  });

  test('clicking a pill opens the popover', async ({ page }) => {
    await page.goto(show('y2026'));
    await page.locator('.pill').first().click();
    expect(await page.evaluate(() => (window as any).__lastOpen)).toBeTruthy();
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `npm --prefix ui run test:ui -- --project=chromium -g "fourteen rows"`
Expected: FAIL — no such component.

- [ ] **Step 3: Implement**

Fourteen `.rrow`s of 28 `.rday`s. A `.rday` gets `.wknd` on columns 5,6,12,13,19,20,26,27, `.out` when `!in_year`, `.unsynced` when `unsynced`. Month starts get an inline chip on day 1.

Pills are positioned from `Lane.start_col`/`end_col`, coloured by their event's calendar colour, and carry `.cont` when `cont_left || cont_right` — **read the flags, do not recompute them**. The legend lists `payload.legend`.

`.out` days are **dimmed, not blank** — the grid stays rectangular, which is what makes the stripes readable.

- [ ] **Step 4: Add the command and binding**

`get_big_year(year)` fetches `events_in_window` across the 392-day ribbon widened a day either side and calls `assemble_big_year`. **Register it in `invoke_handler`.**

- [ ] **Step 5: Verify and prove the tests guard**

Full suites plus `check` and clippy. Then: shade weekends by a per-row modulo that drifts (`(i + r) % 7 >= 5`) and confirm the stripe spec fails. Render `.out` days as empty and confirm the dimming spec fails. Restore each with `diff`, **asserting each applied first**.

- [ ] **Step 6: Commit**

```bash
git add src-tauri ui
git commit -m "feat(ui): the big year ribbon"
```

---

### Task 5: Light up the switcher

**Files:**
- Modify: `ui/src/lib/ViewSwitcher.svelte`, `ui/src/App.svelte`, `ui/tests/app.spec.ts`

**Interfaces:**
- Consumes: `getYear`, `getBigYear`.

**THE CARRY-FORWARD THAT MAKES THIS ONE COMMIT.** Today `step()` returns early for `year`/`bigyear` while `‹`/`›` would announce "Previous year"/"Next year" from the `NAV_UNIT` map. Enabling the slots without giving `step()` a year branch ships arrows that name motion they do not perform. **Both halves land together or neither does.**

- [ ] **Step 1: Write the failing specs**

```ts
// ui/tests/app.spec.ts

test('all five views are reachable', async ({ page }) => {
  await page.goto(app('connected'));
  await expect(page.locator('.vswitch button[disabled]')).toHaveCount(0);
  await page.keyboard.press('4');
  await expect(page.locator('.ymonth')).toHaveCount(12);
  await page.keyboard.press('5');
  await expect(page.locator('.rrow')).toHaveCount(14);
});

test('H and L step by a year in the year views', async ({ page }) => {
  await page.goto(app('connected'));
  await page.keyboard.press('4');
  const before = await page.locator('.ygrid').getAttribute('data-year');
  await page.keyboard.press('l');
  const after = await page.locator('.ygrid').getAttribute('data-year');
  expect(Number(after)).toBe(Number(before) + 1);
});

test('big year reaches this year and next, and no further back', async ({ page }) => {
  // Spec §4: it is a planning surface — what is coming, not what happened.
  await page.goto(app('connected'));
  await page.keyboard.press('5');
  const opened = await page.locator('.ribbon').getAttribute('data-year');
  await page.keyboard.press('h');
  expect(await page.locator('.ribbon').getAttribute('data-year')).toBe(opened);
  await page.keyboard.press('l');
  expect(Number(await page.locator('.ribbon').getAttribute('data-year'))).toBe(Number(opened) + 1);
  await page.keyboard.press('l');
  expect(Number(await page.locator('.ribbon').getAttribute('data-year'))).toBe(Number(opened) + 1);
});

test('the arrows step a year too, and say so', async ({ page }) => {
  await page.goto(app('connected'));
  await page.keyboard.press('4');
  await expect(page.getByRole('button', { name: 'Next year' })).toBeVisible();
  const before = await page.locator('.ygrid').getAttribute('data-year');
  await page.getByRole('button', { name: 'Next year' }).click();
  expect(Number(await page.locator('.ygrid').getAttribute('data-year'))).toBe(Number(before) + 1);
});
```

`YearGrid` needs `data-year` on `.ygrid` and `BigYearRibbon` needs it on `.ribbon` for these to be assertable — add them in Tasks 2 and 4 if you are reading out of order.

- [ ] **Step 2: Run to verify failure**

Run: `npm --prefix ui run test:ui -- --project=chromium -g "all five views"`
Expected: FAIL — two buttons still disabled.

- [ ] **Step 3: Implement**

Drop `disabled: true` from both `SLOTS` entries and remove them from `DISABLED_VIEWS`. Give `step()` a year branch: `year` steps freely in both directions; `bigyear` clamps to `[currentYear, currentYear + 1]` per spec §4 — it is a planning surface, and bounding it also keeps it inside what the sync window plausibly holds.

- [ ] **Step 4: Verify and prove the tests guard**

Full suites plus `check` and clippy. Then: let `bigyear` step below the current year and confirm the bound spec fails. Make `step()` return early for `year` again — leaving the arrow labels in place, so the failure proves motion rather than text — and confirm the arrow spec fails. Restore each with `diff`, **asserting each applied first**.

- [ ] **Step 5: Commit**

```bash
git add ui
git commit -m "feat(ui): year and big year join the switcher"
```

---

## Definition of Done

- [ ] All five views render and the switcher reaches every one — no disabled slots
- [ ] `4` and `5` switch to Year and Big Year; `H`/`L` step a year in both
- [ ] Big Year reaches this year and next only; `‹` does nothing on the current year
- [ ] The arrows step a year in the year views and are labelled accordingly
- [ ] Year shows twelve months with correct day counts, leap Februaries, and lead blanks
- [ ] Only all-day events dot the Year grid
- [ ] Big Year's weekend columns are `[5,6,12,13,19,20,26,27]` in **every** row
- [ ] A span crossing a ribbon row splits, and both halves carry a continuation marker
- [ ] Days outside the synced window render distinctly from in-window days with nothing on
- [ ] Clicking a pill opens the popover; clicking a Year date opens Day view
- [ ] The `#[cfg(test)]` `day_boundaries` shim is gone
- [ ] `cargo test --workspace` ≥ 268, `npm --prefix ui run test:ui` ≥ 300, clippy and `check` clean

## Deliberately not in this plan

- The filmstrip toggle, search, and keyboard beyond `1`-`5` / `H`/`L`/`T`.
- Creating, editing or deleting events.
- Notifications.
- Widening the sync window or fetching on demand. §6's requirement is that
  unfetched dates *say so*, not that they be fetched — that remains a separate
  decision.
