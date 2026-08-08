# All-Day Placement — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** Draw an all-day event in the column for the day it is actually on.

**Base:** `main` @ `0245641` — 396 Rust tests, 488 UI tests.

## The defect

Recorded as §7.1 of `docs/superpowers/specs/2026-08-08-omacal-form-time-boundary-design.md`, found by Plan 6's whole-branch review.

`commands::assemble_days` places an all-day event with
`signed_column(&bounds, iv.start_ms)`, where `bounds = n_day_boundaries(start_ms, n, tz)` and `tz` is `lib.rs::display_tz` — the **system** zone. But the stored instant is midnight in the **calendar's** zone.

Display `Europe/Sofia` (+3), calendar `Pacific/Auckland` (+12), an all-day event on 10 Aug: the stored start `2026-08-09T12:00Z` falls inside Sofia's 9 Aug bounds, so the chip is drawn under **Sun 9** — while its own popover says **Mon, Aug 10** and the form opens on **2026-08-10**.

**No data risk.** Plan 6 made the writes correct. This is placement only. But before Plan 6 all three surfaces were wrong together, so closing two of them turned a consistent error into a visible contradiction.

## The design

**An all-day event has a date, not an instant — so place it by matching dates.**

That is Plan 6's own conclusion applied one layer out. Comparing instants against day boundaries is the thing that went wrong; comparing a date to a date cannot.

- Its date is `date_in_zone(start_utc, calendar_timezone)` — already the derivation `EventDetail` uses (`events.rs::all_day_dates`).
- A column's date is that column's own civil date in the **display** zone.
- The chip goes where those two strings are equal. A multi-day event spans from its start date's column to its (inclusive) end date's column.

Timed events are unchanged — they genuinely are instants and bucketing them by the display zone is right.

## Global Constraints

- **Never** modify `~/Library/Application Support/com.omacal.app/omacal.db` or `~/.config/omacal/config.toml` (three files each — `-wal` and `-shm` too).
- `sqlx::query`/`query_as`/`query_scalar` only — **never** the `query!` macros.
- Svelte 5 runes only. **Never** `{@html}`. No new hardcoded hex.
- **No live network calls in tests.** Playwright specs depending on "now" must call `page.clock.setFixedTime`.
- **Every test shown to fail against broken code, with the mutation asserted on disk (`grep -F`, as its own statement, with a COUNT or line match) before the suite runs.** Revert with a targeted `Edit` — never `git checkout -- <file>`, never `perl`.
- **Verify gates by exit code**, output to a fresh directory. Rust is `cargo test --workspace` (396); `src-tauri/` alone is 263 — say which.

## Fixture rules this project has paid for

Choosing wrongly gives a green test over a live bug — that is how three defects survived ten tasks of Plan 5.

- **A fixture needs the right zone premise AND the call shape the app makes.** Two of Plan 6's had the zone right and entered through a signature the app never calls; they proved nothing.
- **For this plan specifically:** the calendar's zone must put midnight on a **different UTC date** *and* on a different date from the display zone. `Pacific/Auckland` (+12) against a UTC or Sofia display separates. A display zone equal to the calendar's cannot see this at all.
- **Assert the fixture's own premise** — the stored instant, the offsets — so it fails rather than passes vacuously if it stops separating.
- **One fixture may not close two survivors.** Check the arithmetic.

---

### Task 1: the window query carries the calendar's zone

**Files:** `crates/omacal-store/src/events.rs`

`SELECT_COLS` already joins `calendars` for `c.color_hex`, so `c.timezone` is one more column on a row already fetched — no extra round trip. `event_row_for_write` already selects it and documents why it needs no alias (`events` has no column of that name).

`StoredEvent` gains `calendar_timezone: String`.

- [ ] **Step 1:** a store test asserting `events_in_window` returns the **calendar's** zone, not the event's `start_tz`. Seed a calendar in `Pacific/Auckland` with an event whose own `start_tz` is `UTC`, so reading the wrong column fails.
- [ ] **Step 2:** run, confirm it fails.
- [ ] **Step 3:** implement.
- [ ] **Step 4:** run, confirm it passes.
- [ ] **Step 5:** mutate the SELECT to `e.start_tz AS timezone`; assert on disk with a count; confirm the test fails; revert with an `Edit`.
- [ ] **Step 6:** commit.

### Task 2: place all-day events by date

**Files:** `src-tauri/src/commands.rs`

`assemble_days` and `assemble_month` both bucket all-day events. Both move to date matching. Timed events keep `signed_column` unchanged.

- [ ] **Step 1:** tests. At minimum: a single-day all-day event on an Auckland calendar rendered against a UTC display lands in **its own** column, not the previous one; a multi-day event spans the right columns; a timed event on the same calendar is **unchanged**; and the month grid agrees with the week grid for the same event.
- [ ] **Step 2:** run, confirm they fail.
- [ ] **Step 3:** implement. `date_in_zone` lives in `write.rs`; reuse it rather than writing a second derivation — this project has twice nearly shipped a duplicate date formatter.
- [ ] **Step 4:** run, confirm they pass.
- [ ] **Step 5:** mutate the placement back to `signed_column(&bounds, iv.start_ms)`; assert on disk; confirm each test fails; revert. Then mutate the *timed* path the same way and confirm it does **not** fail an all-day test — the two must be separately witnessed.
- [ ] **Step 6:** commit.

### Task 3: the grid agrees with the popover, end to end

**Files:** `ui/tests/` — fixtures and specs

Plan 6 deliberately did not write a grid↔popover agreement spec, because the app fixtures were all UTC and it would have passed vacuously. Task 1 and 2 make it meaningful.

- [ ] **Step 1:** an app-level fixture with an all-day event on a foreign-zone calendar; a spec asserting the column the chip is drawn in and the day its popover names are **the same day**, and that both are the calendar's date.
- [ ] **Step 2–4:** run failing, implement fixtures, run passing.
- [ ] **Step 5:** mutate the Rust placement back and confirm this spec fails — it is the only end-to-end witness.
- [ ] **Step 6:** commit.

---

## Definition of Done

- [ ] `cargo test --workspace` ≥ 400, 0 failed — **by exit code**
- [ ] `npm --prefix ui run test:ui` ≥ 492 — **by exit code**
- [ ] `npm --prefix ui run check` 0 errors; `cargo clippy --workspace --all-targets -- -D warnings` clean
- [ ] The grid, the popover and the form all name the **same day** for an all-day event on a foreign-zone calendar, with one spec proving it
- [ ] §7.1 of the form-time-boundary design doc updated to record this closed
- [ ] Timed placement demonstrably unchanged — its own witness, not the absence of a failure

> **On the bars:** estimates. If the honest number is lower, report the real one — do not pad the suite and do not lower the bar. Both have been tried on this project and both were caught.
