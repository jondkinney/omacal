# omacal M0–M1: Foundation & Read-Only Week View — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An app that authenticates against Google Calendar, syncs your events into local SQLite, and renders your real week in a themed, minimal grid.

**Architecture:** A Cargo workspace of small single-responsibility crates behind a Tauri v2 shell. All hard logic (overlap geometry, lane packing, recurrence expansion) lives in `omacal-core` as pure functions over `i64` epoch milliseconds — no I/O, no datetime library, no browser — so it is unit- and property-testable in isolation. The Svelte UI receives computed geometry and only paints it.

**Tech Stack:** Rust (Tauri v2, sqlx + SQLite, reqwest, jiff, rrule, wiremock), TypeScript + Svelte 5 + Vite.

**Spec:** `docs/superpowers/specs/2026-08-05-omacal-design.md`

## Global Constraints

- **Rust edition 2021**, resolver 2. Workspace at repo root; members are `crates/*` and `src-tauri`.
- **Time is `i64` epoch milliseconds** everywhere inside `omacal-core`. No `jiff`, `chrono` or `time` types cross a public `omacal-core` boundary except in `omacal-core::recur` (see Task 4).
- **`jiff` is the app's datetime library.** `chrono` is permitted *only* as an implementation detail inside `omacal-core::recur`, because `rrule` requires it. It must not appear in any other crate's `Cargo.toml`.
- **Timestamps are stored as UTC instants plus the originating IANA zone**, separately for start and end. Never store a bare local time, and never collapse `end_tz` into `start_tz` — a flight departs in one zone and lands in another.
- **Day-boundary arithmetic always goes through `day_boundaries` (Task 11), never `n * 86_400_000`.** A day containing a DST transition is 23 or 25 hours long; fixed 24-hour arithmetic misplaces events and distorts block geometry.
- **Use `sqlx::query`/`query_as` (runtime-checked), never the `query!` macros.** The macros require a live `DATABASE_URL` at compile time, which breaks a clean `cargo test` on a fresh checkout.
- **Tokens never touch SQLite.** They go to the OS keyring via the `keyring` crate.
- **The app must start even if the theme cannot be parsed** — fall back to the built-in dark palette and log a warning (spec §10).
- **OAuth scope is exactly** `https://www.googleapis.com/auth/calendar`.
- **No live network calls in tests.** `omacal-google` is tested against `wiremock`.
- Crate names: `omacal-core`, `omacal-store`, `omacal-google`, `omacal-sync`, and the Tauri shell in `src-tauri` is package **`omacal`** with lib target `omacal_lib`. (`omacal-notify` arrives in Plan 3.) Rename the generated `src-tauri` package in Task 1 — every later task depends on it.
- Development happens on macOS; Task 15 is the Omarchy verification checkpoint.

---

## Timezone behaviour

In scope for this plan:

| Behaviour | Where |
| --- | --- |
| Events stored as UTC instants + originating IANA zone, start and end separately | Task 5, 6, 9 |
| All-day events resolved against the calendar's zone (midnight in Sofia ≠ midnight UTC) | Task 9 |
| Recurring series anchored to local wall-clock time, so "09:00 every Monday" stays 09:00 across a DST transition | Task 4 |
| Day columns and block geometry computed from true day lengths, so a 23- or 25-hour day renders correctly | Task 11 |
| Display in the system zone | Task 11 |

Deferred, and why:

- **Per-end zone display** ("09:00 IST – 13:00 EET" on a flight). The data is stored correctly from M1, so this is a rendering change with no migration. Plan 2.
- **Timezone override** (view the calendar as if you were in another city). Plan 2, alongside the settings surface.
- **Secondary timezone rail** in day/week view. Plan 2.

## File Structure

| Path | Responsibility |
| --- | --- |
| `Cargo.toml` | Workspace root, shared dependency versions |
| `crates/omacal-core/src/lib.rs` | Re-exports; crate has no I/O |
| `crates/omacal-core/src/layout.rs` | `Interval`, `Placed`, `lay_out_day` — vertical overlap columns |
| `crates/omacal-core/src/lanes.rs` | `Segment`, `Lane`, `pack_lanes` — horizontal lane packing |
| `crates/omacal-core/src/recur.rs` | Recurrence expansion; the only place `chrono` exists |
| `crates/omacal-store/src/lib.rs` | Pool construction, migration runner |
| `crates/omacal-store/migrations/0001_init.sql` | Schema from spec §4 |
| `crates/omacal-store/src/events.rs` | Event read/write queries |
| `crates/omacal-google/src/auth.rs` | PKCE loopback OAuth |
| `crates/omacal-google/src/client.rs` | Calendar API v3 calls |
| `crates/omacal-google/src/model.rs` | Wire types (serde) |
| `crates/omacal-sync/src/lib.rs` | Sync orchestration, 410 recovery |
| `src-tauri/src/theme.rs` | Theme resolution + palette |
| `src-tauri/src/commands.rs` | Tauri command surface |
| `ui/src/lib/WeekGrid.svelte` | Week grid shell |
| `ui/src/lib/EventBlock.svelte` | Event block, density + RSVP states |
| `ui/src/lib/AllDayBand.svelte` | All-day band |

---

### Task 1: Workspace scaffold

**Files:**
- Create: `Cargo.toml`, `crates/omacal-core/Cargo.toml`, `crates/omacal-core/src/lib.rs`
- Create: `src-tauri/`, `ui/` (generated)

**Interfaces:**
- Consumes: nothing
- Produces: a workspace where `cargo test` runs and `npm run tauri dev` opens a window on macOS

- [ ] **Step 1: Scaffold the Tauri app**

```bash
cd /Users/plamen/dev/omacal
npm create tauri-app@latest -- --template svelte-ts --manager npm --yes ui-tmp
```

Accept Svelte + TypeScript. This creates `ui-tmp/` containing both a `src-tauri/` and the frontend.

- [ ] **Step 2: Flatten into the repo root**

```bash
mv ui-tmp/src-tauri ./src-tauri
mkdir -p ui && mv ui-tmp/src ui/src && mv ui-tmp/index.html ui/ \
  && mv ui-tmp/package.json ui/ && mv ui-tmp/vite.config.ts ui/ \
  && mv ui-tmp/tsconfig.json ui/ && mv ui-tmp/svelte.config.js ui/
rm -rf ui-tmp
```

Then set `frontendDist` and `devUrl` in `src-tauri/tauri.conf.json` to point at `../ui/dist` and `http://localhost:1420`, and set `beforeDevCommand` to `npm --prefix ../ui run dev`, `beforeBuildCommand` to `npm --prefix ../ui run build`.

- [ ] **Step 3: Create the workspace root manifest**

```toml
# Cargo.toml
[workspace]
resolver = "2"
members = ["crates/*", "src-tauri"]

[workspace.package]
edition = "2021"
rust-version = "1.82"

[workspace.dependencies]
anyhow    = "1"
thiserror = "2"
serde     = { version = "1", features = ["derive"] }
serde_json = "1"
tracing   = "0.1"
```

In `src-tauri/Cargo.toml`, set the package name to **`omacal`** and adopt the workspace edition. Every later task refers to this crate as `omacal` (`cargo add --package omacal`, `cargo test -p omacal`), so the generated name must be changed:

```toml
[package]
name = "omacal"
version = "0.1.0"
edition.workspace = true

[lib]
name = "omacal_lib"
crate-type = ["staticlib", "cdylib", "rlib"]
```

Update `src-tauri/src/main.rs` to call `omacal_lib::run()` to match.

- [ ] **Step 4: Create the core crate**

```toml
# crates/omacal-core/Cargo.toml
[package]
name = "omacal-core"
version = "0.1.0"
edition.workspace = true

[dependencies]
serde.workspace = true
```

```rust
// crates/omacal-core/src/lib.rs
pub mod layout;
```

```rust
// crates/omacal-core/src/layout.rs
// Populated in Task 2.
```

- [ ] **Step 5: Verify both toolchains**

Run: `cargo test --workspace`
Expected: compiles, 0 tests.

Run: `npm --prefix ui install && npm run tauri dev` (from `src-tauri`, or `cargo tauri dev`)
Expected: a window opens on macOS.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "chore: scaffold Tauri v2 + Svelte workspace"
```

---

### Task 2: Overlap column assignment (`lay_out_day`)

This is the algorithm behind spec §7.1 — "columns plus layering". Pure integer maths, no dependencies.

**Files:**
- Modify: `crates/omacal-core/src/layout.rs`

**Interfaces:**
- Consumes: nothing
- Produces:
  ```rust
  pub struct Interval { pub start_ms: i64, pub end_ms: i64 }
  pub struct Placed { pub idx: usize, pub column: u8, pub columns: u8, pub top: f32, pub height: f32 }
  pub fn lay_out_day(events: &[Interval], day_start_ms: i64, day_end_ms: i64) -> Vec<Placed>
  ```
  `top` and `height` are fractions of the day window in `0.0..=1.0`. `column` is 0-based; `columns` is the width of the cluster this event belongs to.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/omacal-core/src/layout.rs
#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 0;
    const END: i64 = 24 * 3_600_000;

    fn iv(h_start: f64, h_end: f64) -> Interval {
        Interval { start_ms: (h_start * 3_600_000.0) as i64, end_ms: (h_end * 3_600_000.0) as i64 }
    }

    #[test]
    fn disjoint_events_each_take_the_full_width() {
        let out = lay_out_day(&[iv(9.0, 10.0), iv(11.0, 12.0)], DAY, END);
        assert_eq!(out[0].columns, 1);
        assert_eq!(out[1].columns, 1);
        assert_eq!(out[0].column, 0);
        assert_eq!(out[1].column, 0);
    }

    #[test]
    fn identical_times_split_evenly() {
        let out = lay_out_day(&[iv(10.0, 11.0), iv(10.0, 11.0)], DAY, END);
        assert_eq!(out[0].columns, 2);
        assert_eq!(out[1].columns, 2);
        assert_eq!(out[0].column, 0);
        assert_eq!(out[1].column, 1);
    }

    #[test]
    fn partial_overlap_splits_the_cluster() {
        // 10:00-11:00 and 10:30-11:30 collide only in the middle half hour.
        let out = lay_out_day(&[iv(10.0, 11.0), iv(10.5, 11.5)], DAY, END);
        assert_eq!(out[0].columns, 2);
        assert_eq!(out[1].column, 1);
    }

    #[test]
    fn three_way_pile_uses_three_columns() {
        let out = lay_out_day(&[iv(10.0, 12.0), iv(10.5, 11.0), iv(10.75, 11.5)], DAY, END);
        assert_eq!(out.iter().map(|p| p.columns).max().unwrap(), 3);
        assert_eq!(out[0].column, 0);
        assert_eq!(out[1].column, 1);
        assert_eq!(out[2].column, 2);
    }

    #[test]
    fn a_gap_starts_a_new_cluster() {
        // First two collide; the third is alone and must not be widened to 2 columns.
        let out = lay_out_day(&[iv(9.0, 10.0), iv(9.5, 10.0), iv(14.0, 15.0)], DAY, END);
        assert_eq!(out[0].columns, 2);
        assert_eq!(out[2].columns, 1);
    }

    #[test]
    fn touching_events_do_not_overlap() {
        // 09:00-10:00 and 10:00-11:00 share only an instant; that is not a collision.
        let out = lay_out_day(&[iv(9.0, 10.0), iv(10.0, 11.0)], DAY, END);
        assert_eq!(out[0].columns, 1);
        assert_eq!(out[1].columns, 1);
    }

    #[test]
    fn geometry_is_a_fraction_of_the_window() {
        let out = lay_out_day(&[iv(6.0, 12.0)], DAY, END);
        assert!((out[0].top - 0.25).abs() < 1e-6);
        assert!((out[0].height - 0.25).abs() < 1e-6);
    }

    #[test]
    fn events_are_clamped_to_the_window() {
        let out = lay_out_day(&[iv(-2.0, 2.0)], DAY, END);
        assert!((out[0].top - 0.0).abs() < 1e-6);
        assert!(out[0].height > 0.0);
    }

    #[test]
    fn zero_length_events_still_get_a_visible_height() {
        let out = lay_out_day(&[iv(9.0, 9.0)], DAY, END);
        assert!(out[0].height > 0.0);
    }

    /// The invariant that matters: two events may never share a column while
    /// overlapping in time.
    #[test]
    fn no_two_events_share_a_column_while_overlapping() {
        let evs: Vec<Interval> = (0..40)
            .map(|i| {
                let s = (i as i64 * 7 % 20) * 1_800_000;
                Interval { start_ms: s, end_ms: s + ((i as i64 % 5) + 1) * 1_800_000 }
            })
            .collect();
        let out = lay_out_day(&evs, DAY, END);
        for a in &out {
            for b in &out {
                if a.idx == b.idx || a.column != b.column {
                    continue;
                }
                let (x, y) = (&evs[a.idx], &evs[b.idx]);
                assert!(
                    x.start_ms >= y.end_ms || y.start_ms >= x.end_ms,
                    "events {} and {} overlap but share column {}", a.idx, b.idx, a.column
                );
            }
        }
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p omacal-core`
Expected: FAIL — `cannot find function lay_out_day`.

- [ ] **Step 3: Implement**

```rust
// crates/omacal-core/src/layout.rs  (above the tests module)
use serde::Serialize;

/// A half-open time interval in epoch milliseconds: `[start_ms, end_ms)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Interval {
    pub start_ms: i64,
    pub end_ms: i64,
}

impl Interval {
    fn overlaps(&self, other: &Interval) -> bool {
        self.start_ms < other.end_ms && other.start_ms < self.end_ms
    }
}

/// Computed geometry for one event in a day column.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Placed {
    /// Index into the input slice.
    pub idx: usize,
    /// 0-based column within this event's cluster.
    pub column: u8,
    /// Total columns in this event's cluster.
    pub columns: u8,
    /// Fraction of the day window, 0.0..=1.0.
    pub top: f32,
    /// Fraction of the day window; always > 0.
    pub height: f32,
}

/// Minimum rendered height, as a fraction of the window, so that a zero-length
/// or very short event is still clickable.
const MIN_HEIGHT: f32 = 0.004;

/// Assigns overlapping events to side-by-side columns and computes vertical
/// geometry as fractions of `[day_start_ms, day_end_ms)`.
///
/// Events are grouped into *clusters*: maximal runs of events connected by
/// overlap. Every event in a cluster is rendered at the cluster's full column
/// count so that edges line up, which is what makes a collision read as one.
pub fn lay_out_day(events: &[Interval], day_start_ms: i64, day_end_ms: i64) -> Vec<Placed> {
    if events.is_empty() {
        return Vec::new();
    }

    // Sort indices by start, then by longest-first so big blocks take column 0.
    let mut order: Vec<usize> = (0..events.len()).collect();
    order.sort_by_key(|&i| {
        let e = events[i];
        (e.start_ms, -(e.end_ms - e.start_ms), i as i64)
    });

    let mut column_of = vec![0u8; events.len()];
    let mut columns_of = vec![1u8; events.len()];

    // Events currently occupying each column, as (end_ms, idx).
    let mut active: Vec<Option<(i64, usize)>> = Vec::new();
    let mut cluster: Vec<usize> = Vec::new();
    let mut cluster_end: i64 = i64::MIN;

    for &i in &order {
        let ev = events[i];

        // A cluster ends when an event starts at or after every active event's end.
        if !cluster.is_empty() && ev.start_ms >= cluster_end {
            let width = active.len() as u8;
            for &c in &cluster {
                columns_of[c] = width;
            }
            cluster.clear();
            active.clear();
            cluster_end = i64::MIN;
        }

        // First column whose occupant has already finished.
        let slot = active
            .iter()
            .position(|s| s.map_or(true, |(end, _)| end <= ev.start_ms));

        let col = match slot {
            Some(c) => {
                active[c] = Some((ev.end_ms, i));
                c
            }
            None => {
                active.push(Some((ev.end_ms, i)));
                active.len() - 1
            }
        };

        column_of[i] = col as u8;
        cluster.push(i);
        cluster_end = cluster_end.max(ev.end_ms);
    }

    if !cluster.is_empty() {
        let width = active.len() as u8;
        for &c in &cluster {
            columns_of[c] = width;
        }
    }

    let span = (day_end_ms - day_start_ms).max(1) as f32;
    events
        .iter()
        .enumerate()
        .map(|(idx, ev)| {
            let start = ev.start_ms.clamp(day_start_ms, day_end_ms);
            let end = ev.end_ms.clamp(start, day_end_ms);
            let top = (start - day_start_ms) as f32 / span;
            let height = ((end - start) as f32 / span).max(MIN_HEIGHT);
            Placed { idx, column: column_of[idx], columns: columns_of[idx], top, height }
        })
        .collect()
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p omacal-core`
Expected: 10 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/omacal-core
git commit -m "feat(core): overlap column assignment for day layout"
```

---

### Task 3: Lane packing (`pack_lanes`)

Serves the week view's all-day band, the month view's rows, and both year views (spec §7.4, §7.5). Horizontal analogue of Task 2.

**Files:**
- Create: `crates/omacal-core/src/lanes.rs`
- Modify: `crates/omacal-core/src/lib.rs`

**Interfaces:**
- Consumes: nothing
- Produces:
  ```rust
  pub struct Segment { pub idx: usize, pub start_col: i32, pub end_col: i32 }
  pub struct Lane { pub idx: usize, pub lane: u8, pub start_col: u16, pub end_col: u16,
                    pub cont_left: bool, pub cont_right: bool }
  pub fn pack_lanes(segs: &[Segment], row_len: u16, max_lanes: u8) -> (Vec<Lane>, Vec<usize>)
  ```
  `start_col`/`end_col` on input are **inclusive** and may fall outside `0..row_len` — they are clipped, and the clipping sets `cont_left`/`cont_right`. Returns placed lanes plus the indices that overflowed past `max_lanes`.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/omacal-core/src/lanes.rs
#[cfg(test)]
mod tests {
    use super::*;

    fn seg(idx: usize, a: i32, b: i32) -> Segment {
        Segment { idx, start_col: a, end_col: b }
    }

    #[test]
    fn a_single_segment_takes_lane_zero() {
        let (lanes, over) = pack_lanes(&[seg(0, 2, 5)], 28, 3);
        assert_eq!(lanes[0].lane, 0);
        assert_eq!(lanes[0].start_col, 2);
        assert_eq!(lanes[0].end_col, 5);
        assert!(over.is_empty());
    }

    #[test]
    fn non_overlapping_segments_share_lane_zero() {
        let (lanes, _) = pack_lanes(&[seg(0, 0, 3), seg(1, 5, 8)], 28, 3);
        assert_eq!(lanes[0].lane, 0);
        assert_eq!(lanes[1].lane, 0);
    }

    #[test]
    fn adjacent_segments_share_a_lane() {
        // Inclusive columns: 0..=3 and 4..=6 touch but do not overlap.
        let (lanes, _) = pack_lanes(&[seg(0, 0, 3), seg(1, 4, 6)], 28, 3);
        assert_eq!(lanes[0].lane, 0);
        assert_eq!(lanes[1].lane, 0);
    }

    #[test]
    fn overlapping_segments_stack_into_lanes() {
        let (lanes, _) = pack_lanes(&[seg(0, 0, 5), seg(1, 3, 8)], 28, 3);
        assert_eq!(lanes[0].lane, 0);
        assert_eq!(lanes[1].lane, 1);
    }

    #[test]
    fn longest_segment_wins_lane_zero() {
        // Declared shortest-first, but the long one should sit on top.
        let (lanes, _) = pack_lanes(&[seg(0, 3, 4), seg(1, 0, 20)], 28, 3);
        let long = lanes.iter().find(|l| l.idx == 1).unwrap();
        assert_eq!(long.lane, 0);
    }

    #[test]
    fn segments_are_clipped_to_the_row_and_flagged() {
        let (lanes, _) = pack_lanes(&[seg(0, -5, 40)], 28, 3);
        assert_eq!(lanes[0].start_col, 0);
        assert_eq!(lanes[0].end_col, 27);
        assert!(lanes[0].cont_left);
        assert!(lanes[0].cont_right);
    }

    #[test]
    fn a_segment_inside_the_row_is_not_flagged() {
        let (lanes, _) = pack_lanes(&[seg(0, 1, 26)], 28, 3);
        assert!(!lanes[0].cont_left);
        assert!(!lanes[0].cont_right);
    }

    #[test]
    fn segments_entirely_outside_the_row_are_dropped() {
        let (lanes, over) = pack_lanes(&[seg(0, 40, 50), seg(1, -9, -2)], 28, 3);
        assert!(lanes.is_empty());
        assert!(over.is_empty());
    }

    #[test]
    fn overflow_beyond_max_lanes_is_reported_not_placed() {
        let segs: Vec<Segment> = (0..5).map(|i| seg(i, 0, 10)).collect();
        let (lanes, over) = pack_lanes(&segs, 28, 3);
        assert_eq!(lanes.len(), 3);
        assert_eq!(over.len(), 2);
    }

    /// The invariant: two segments may never share a lane while overlapping.
    #[test]
    fn no_two_segments_share_a_lane_while_overlapping() {
        let segs: Vec<Segment> = (0..30)
            .map(|i| {
                let a = (i as i32 * 3) % 25;
                seg(i, a, a + (i as i32 % 6))
            })
            .collect();
        let (lanes, _) = pack_lanes(&segs, 28, 8);
        for a in &lanes {
            for b in &lanes {
                if a.idx == b.idx || a.lane != b.lane {
                    continue;
                }
                assert!(
                    a.end_col < b.start_col || b.end_col < a.start_col,
                    "segments {} and {} overlap but share lane {}", a.idx, b.idx, a.lane
                );
            }
        }
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p omacal-core lanes`
Expected: FAIL — `cannot find function pack_lanes`.

- [ ] **Step 3: Implement**

```rust
// crates/omacal-core/src/lanes.rs  (above the tests module)
use serde::Serialize;

/// An event clipped to one row, in **inclusive** column indices. Values may
/// fall outside the row; `pack_lanes` clips them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    pub idx: usize,
    pub start_col: i32,
    pub end_col: i32,
}

/// A placed segment.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Lane {
    pub idx: usize,
    pub lane: u8,
    /// Inclusive, clipped to the row.
    pub start_col: u16,
    /// Inclusive, clipped to the row.
    pub end_col: u16,
    /// True when the event began before this row (render a flat left edge).
    pub cont_left: bool,
    /// True when the event continues past this row (render a flat right edge).
    pub cont_right: bool,
}

/// Greedily packs segments into horizontal lanes, longest first, so that the
/// most significant spans sit closest to the day numbers.
///
/// Returns `(placed, overflowed)`, where `overflowed` holds the input indices
/// that did not fit within `max_lanes` — the caller renders them as "+N more".
pub fn pack_lanes(segs: &[Segment], row_len: u16, max_lanes: u8) -> (Vec<Lane>, Vec<usize>) {
    if row_len == 0 || max_lanes == 0 {
        return (Vec::new(), Vec::new());
    }
    let last = (row_len - 1) as i32;

    // Clip to the row, dropping anything fully outside it.
    let mut clipped: Vec<Lane> = segs
        .iter()
        .filter(|s| s.end_col >= 0 && s.start_col <= last && s.start_col <= s.end_col)
        .map(|s| Lane {
            idx: s.idx,
            lane: 0,
            start_col: s.start_col.max(0) as u16,
            end_col: s.end_col.min(last) as u16,
            cont_left: s.start_col < 0,
            cont_right: s.end_col > last,
        })
        .collect();

    // Longest first, then leftmost, then by index for determinism.
    clipped.sort_by_key(|l| {
        let width = l.end_col as i32 - l.start_col as i32;
        (-width, l.start_col, l.idx)
    });

    let mut lanes: Vec<Vec<(u16, u16)>> = Vec::new();
    let mut placed: Vec<Lane> = Vec::new();
    let mut overflow: Vec<usize> = Vec::new();

    'next: for mut item in clipped {
        for (n, occupied) in lanes.iter_mut().enumerate() {
            let free = occupied
                .iter()
                .all(|&(a, b)| item.end_col < a || b < item.start_col);
            if free {
                occupied.push((item.start_col, item.end_col));
                item.lane = n as u8;
                placed.push(item);
                continue 'next;
            }
        }
        if lanes.len() < max_lanes as usize {
            lanes.push(vec![(item.start_col, item.end_col)]);
            item.lane = (lanes.len() - 1) as u8;
            placed.push(item);
        } else {
            overflow.push(item.idx);
        }
    }

    placed.sort_by_key(|l| (l.lane, l.start_col));
    overflow.sort_unstable();
    (placed, overflow)
}
```

- [ ] **Step 4: Wire into the crate root**

```rust
// crates/omacal-core/src/lib.rs
pub mod lanes;
pub mod layout;

pub use lanes::{pack_lanes, Lane, Segment};
pub use layout::{lay_out_day, Interval, Placed};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p omacal-core`
Expected: 20 passed.

- [ ] **Step 6: Commit**

```bash
git add crates/omacal-core
git commit -m "feat(core): horizontal lane packing for multi-day spans"
```

---

### Task 4: Recurrence expansion

The only place `chrono` is allowed. We hand `rrule` an RFC 5545 string and read timestamps *out* — we never construct a chrono datetime from local fields, so the ambiguous-local-time hazard that motivated choosing `jiff` never arises at this boundary.

**Files:**
- Create: `crates/omacal-core/src/recur.rs`
- Modify: `crates/omacal-core/src/lib.rs`, `crates/omacal-core/Cargo.toml`

**Interfaces:**
- Consumes: nothing
- Produces:
  ```rust
  pub struct Series<'a> {
      pub dtstart_ms: i64,
      pub dtstart_tz: &'a str,       // IANA name, e.g. "Europe/Sofia"
      pub duration_ms: i64,
      pub is_all_day: bool,
      pub recurrence: &'a [String],  // raw RRULE/EXDATE/RDATE lines from Google
  }
  pub struct Expansion { pub intervals: Vec<Interval>, pub truncated: bool }
  pub fn expand(series: &Series, from_ms: i64, to_ms: i64, limit: u16)
      -> Result<Expansion, RecurError>
  ```

- [ ] **Step 1: Add dependencies**

```bash
cargo add --package omacal-core rrule chrono thiserror
```

- [ ] **Step 2: Write the failing tests**

```rust
// crates/omacal-core/src/recur.rs
#[cfg(test)]
mod tests {
    use super::*;

    /// Monday 2026-08-03 09:00:00 Europe/Sofia == 06:00:00Z (EEST, UTC+3).
    /// Verify with: `python3 -c "import datetime as d; print(int(d.datetime(2026,8,3,6,tzinfo=d.timezone.utc).timestamp()*1000))"`
    const MON_0900_SOFIA: i64 = 1_785_736_800_000;
    const HOUR: i64 = 3_600_000;
    const DAY: i64 = 24 * HOUR;

    fn weekly(rules: &[&str]) -> Vec<String> {
        rules.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_non_recurring_event_yields_itself_when_in_window() {
        let s = Series {
            dtstart_ms: MON_0900_SOFIA, dtstart_tz: "Europe/Sofia",
            duration_ms: 30 * 60_000, is_all_day: false, recurrence: &[],
        };
        let out = expand(&s, MON_0900_SOFIA - DAY, MON_0900_SOFIA + DAY, 50).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].start_ms, MON_0900_SOFIA);
        assert_eq!(out[0].end_ms, MON_0900_SOFIA + 30 * 60_000);
    }

    #[test]
    fn a_non_recurring_event_outside_the_window_yields_nothing() {
        let s = Series {
            dtstart_ms: MON_0900_SOFIA, dtstart_tz: "Europe/Sofia",
            duration_ms: 30 * 60_000, is_all_day: false, recurrence: &[],
        };
        let out = expand(&s, MON_0900_SOFIA + 10 * DAY, MON_0900_SOFIA + 20 * DAY, 50).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn a_daily_standup_yields_one_per_day() {
        let s = Series {
            dtstart_ms: MON_0900_SOFIA, dtstart_tz: "Europe/Sofia",
            duration_ms: 30 * 60_000, is_all_day: false,
            recurrence: &weekly(&["RRULE:FREQ=DAILY"]),
        };
        // Window covering Mon..Fri inclusive.
        let out = expand(&s, MON_0900_SOFIA - HOUR, MON_0900_SOFIA + 5 * DAY, 50).unwrap();
        assert_eq!(out.len(), 5);
        assert_eq!(out[0].start_ms, MON_0900_SOFIA);
        assert_eq!(out[1].start_ms, MON_0900_SOFIA + DAY);
    }

    #[test]
    fn every_instance_keeps_the_series_duration() {
        let s = Series {
            dtstart_ms: MON_0900_SOFIA, dtstart_tz: "Europe/Sofia",
            duration_ms: 90 * 60_000, is_all_day: false,
            recurrence: &weekly(&["RRULE:FREQ=DAILY;COUNT=3"]),
        };
        let out = expand(&s, MON_0900_SOFIA - HOUR, MON_0900_SOFIA + 5 * DAY, 50).unwrap();
        for i in &out {
            assert_eq!(i.end_ms - i.start_ms, 90 * 60_000);
        }
    }

    #[test]
    fn exdate_removes_an_instance() {
        let s = Series {
            dtstart_ms: MON_0900_SOFIA, dtstart_tz: "Europe/Sofia",
            duration_ms: 30 * 60_000, is_all_day: false,
            recurrence: &weekly(&[
                "RRULE:FREQ=DAILY",
                // Tuesday 2026-08-04 09:00 Sofia == 06:00Z
                "EXDATE;TZID=Europe/Sofia:20260804T090000",
            ]),
        };
        let out = expand(&s, MON_0900_SOFIA - HOUR, MON_0900_SOFIA + 3 * DAY, 50).unwrap();
        assert!(out.iter().all(|i| i.start_ms != MON_0900_SOFIA + DAY));
    }

    #[test]
    fn count_is_respected() {
        let s = Series {
            dtstart_ms: MON_0900_SOFIA, dtstart_tz: "Europe/Sofia",
            duration_ms: 30 * 60_000, is_all_day: false,
            recurrence: &weekly(&["RRULE:FREQ=DAILY;COUNT=2"]),
        };
        let out = expand(&s, MON_0900_SOFIA - HOUR, MON_0900_SOFIA + 30 * DAY, 50).unwrap();
        assert_eq!(out.len(), 2);
    }

    /// The reason `jiff`/IANA zones matter. Europe/Sofia leaves DST on
    /// 2026-10-25. A 09:00 local weekly meeting must stay at 09:00 local,
    /// which means its UTC instant shifts by an hour across the boundary.
    #[test]
    fn a_local_time_series_survives_a_dst_transition() {
        // Monday 2026-09-28 09:00 Sofia (EEST, +3) == 06:00Z
        let sep28 = 1_790_575_200_000;
        let s = Series {
            dtstart_ms: sep28, dtstart_tz: "Europe/Sofia",
            duration_ms: HOUR, is_all_day: false,
            recurrence: &weekly(&["RRULE:FREQ=WEEKLY;BYDAY=MO"]),
        };
        let out = expand(&s, sep28 - HOUR, sep28 + 45 * DAY, 50).unwrap();
        let deltas: Vec<i64> = out.windows(2).map(|w| w[1].start_ms - w[0].start_ms).collect();
        // Exactly one gap is 7 days + 1 hour: the week DST ends.
        assert_eq!(deltas.iter().filter(|&&d| d == 7 * DAY + HOUR).count(), 1,
                   "expected one DST-adjusted gap, got {:?}", deltas);
        assert!(deltas.iter().all(|&d| d == 7 * DAY || d == 7 * DAY + HOUR));
    }

    #[test]
    fn the_limit_caps_runaway_expansion() {
        let s = Series {
            dtstart_ms: MON_0900_SOFIA, dtstart_tz: "Europe/Sofia",
            duration_ms: 30 * 60_000, is_all_day: false,
            recurrence: &weekly(&["RRULE:FREQ=MINUTELY"]),
        };
        let out = expand(&s, MON_0900_SOFIA, MON_0900_SOFIA + DAY, 10).unwrap();
        assert_eq!(out.len(), 10);
    }

    #[test]
    fn a_malformed_rule_is_an_error_not_a_panic() {
        let s = Series {
            dtstart_ms: MON_0900_SOFIA, dtstart_tz: "Europe/Sofia",
            duration_ms: 30 * 60_000, is_all_day: false,
            recurrence: &weekly(&["RRULE:FREQ=NONSENSE"]),
        };
        assert!(expand(&s, MON_0900_SOFIA, MON_0900_SOFIA + DAY, 10).is_err());
    }

    #[test]
    fn an_unknown_timezone_is_an_error_not_a_panic() {
        let s = Series {
            dtstart_ms: MON_0900_SOFIA, dtstart_tz: "Mars/Olympus_Mons",
            duration_ms: 30 * 60_000, is_all_day: false,
            recurrence: &weekly(&["RRULE:FREQ=DAILY"]),
        };
        assert!(expand(&s, MON_0900_SOFIA, MON_0900_SOFIA + DAY, 10).is_err());
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p omacal-core recur`
Expected: FAIL — `cannot find function expand`.

- [ ] **Step 4: Implement**

```rust
// crates/omacal-core/src/recur.rs  (above the tests module)
use crate::layout::Interval;
use chrono::TimeZone;
use rrule::{RRuleSet, Tz};
use std::str::FromStr;

#[derive(Debug, thiserror::Error)]
pub enum RecurError {
    #[error("unknown time zone: {0}")]
    UnknownTimeZone(String),
    #[error("invalid recurrence rule: {0}")]
    InvalidRule(String),
    #[error("timestamp out of range: {0}")]
    OutOfRange(i64),
}

/// A recurring (or single) event as stored: a start instant, the IANA zone it
/// was authored in, a duration, and Google's raw recurrence lines.
#[derive(Debug, Clone)]
pub struct Series<'a> {
    pub dtstart_ms: i64,
    pub dtstart_tz: &'a str,
    pub duration_ms: i64,
    pub is_all_day: bool,
    pub recurrence: &'a [String],
}

fn to_chrono(ms: i64) -> Result<chrono::DateTime<Tz>, RecurError> {
    Tz::UTC
        .timestamp_millis_opt(ms)
        .single()
        .ok_or(RecurError::OutOfRange(ms))
}

/// Renders the DTSTART line in the series' own zone, which is what makes a
/// "09:00 every Monday" meeting stay at 09:00 across a DST transition.
fn dtstart_line(series: &Series) -> Result<String, RecurError> {
    let zone: chrono_tz::Tz = series
        .dtstart_tz
        .parse()
        .map_err(|_| RecurError::UnknownTimeZone(series.dtstart_tz.to_string()))?;
    let local = chrono::Utc
        .timestamp_millis_opt(series.dtstart_ms)
        .single()
        .ok_or(RecurError::OutOfRange(series.dtstart_ms))?
        .with_timezone(&zone);

    Ok(if series.is_all_day {
        format!("DTSTART;VALUE=DATE:{}", local.format("%Y%m%d"))
    } else {
        format!(
            "DTSTART;TZID={}:{}",
            series.dtstart_tz,
            local.format("%Y%m%dT%H%M%S")
        )
    })
}

/// Expands `series` into concrete intervals overlapping `[from_ms, to_ms)`.
///
/// `limit` bounds the number of occurrences generated, guarding against
/// unbounded rules such as `FREQ=MINUTELY` with no `COUNT`/`UNTIL`.
pub fn expand(
    series: &Series,
    from_ms: i64,
    to_ms: i64,
    limit: u16,
) -> Result<Vec<Interval>, RecurError> {
    // Validate the zone even when there is no rule, so callers get a
    // consistent error rather than a silent pass.
    let dtstart = dtstart_line(series)?;

    if series.recurrence.is_empty() {
        let end = series.dtstart_ms + series.duration_ms;
        return Ok(if series.dtstart_ms < to_ms && end > from_ms {
            vec![Interval { start_ms: series.dtstart_ms, end_ms: end }]
        } else {
            Vec::new()
        });
    }

    let mut source = String::with_capacity(128);
    source.push_str(&dtstart);
    for line in series.recurrence {
        source.push('\n');
        source.push_str(line.trim());
    }

    let set = RRuleSet::from_str(&source)
        .map_err(|e| RecurError::InvalidRule(format!("{e}: {source}")))?;

    // Widen the query by the event duration so an occurrence that started
    // before the window but is still running is not dropped.
    let query_from = from_ms.saturating_sub(series.duration_ms);
    let result = set
        .after(to_chrono(query_from)?)
        .before(to_chrono(to_ms)?)
        .all(limit);

    Ok(result
        .dates
        .into_iter()
        .map(|d| {
            let start = d.timestamp_millis();
            Interval { start_ms: start, end_ms: start + series.duration_ms }
        })
        .filter(|i| i.start_ms < to_ms && i.end_ms > from_ms)
        .collect())
}
```

Add `chrono-tz` if `rrule` does not already re-export it:

```bash
cargo add --package omacal-core chrono-tz
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p omacal-core recur`
Expected: 10 passed.

If `a_local_time_series_survives_a_dst_transition` fails, the fixture instants are wrong rather than the code — recompute them with `TZ=Europe/Sofia date -d @<seconds>` and correct the constants before touching `expand`.

- [ ] **Step 6: Wire into the crate root and commit**

```rust
// crates/omacal-core/src/lib.rs — add
pub mod recur;
pub use recur::{expand, RecurError, Series};
```

```bash
cargo test -p omacal-core
git add crates/omacal-core
git commit -m "feat(core): RFC 5545 recurrence expansion with DST-safe DTSTART"
```

---

### Task 5: Store schema and migrations

**Files:**
- Create: `crates/omacal-store/Cargo.toml`, `crates/omacal-store/src/lib.rs`
- Create: `crates/omacal-store/migrations/0001_init.sql`

**Interfaces:**
- Consumes: nothing
- Produces:
  ```rust
  pub async fn connect(url: &str) -> anyhow::Result<SqlitePool>   // runs migrations
  pub async fn connect_memory() -> anyhow::Result<SqlitePool>     // for tests
  ```

- [ ] **Step 1: Create the crate**

```bash
cargo new --lib crates/omacal-store
cargo add --package omacal-store sqlx --features runtime-tokio,sqlite,migrate
cargo add --package omacal-store anyhow tokio --features tokio/rt-multi-thread,tokio/macros
```

- [ ] **Step 2: Write the migration**

```sql
-- crates/omacal-store/migrations/0001_init.sql
PRAGMA foreign_keys = ON;

CREATE TABLE accounts (
  id            INTEGER PRIMARY KEY,
  google_sub    TEXT NOT NULL UNIQUE,
  email         TEXT NOT NULL,
  display_name  TEXT,
  created_at    INTEGER NOT NULL
);

CREATE TABLE calendars (
  id           INTEGER PRIMARY KEY,
  account_id   INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  google_id    TEXT NOT NULL,
  summary      TEXT NOT NULL,
  color_hex    TEXT,
  timezone     TEXT NOT NULL,
  access_role  TEXT NOT NULL,
  selected     INTEGER NOT NULL DEFAULT 1,
  is_primary   INTEGER NOT NULL DEFAULT 0,
  UNIQUE (account_id, google_id)
);

CREATE TABLE events (
  id                  INTEGER PRIMARY KEY,
  calendar_id         INTEGER NOT NULL REFERENCES calendars(id) ON DELETE CASCADE,
  google_id           TEXT NOT NULL,
  ical_uid            TEXT,
  etag                TEXT,
  summary             TEXT,
  description         TEXT,
  location            TEXT,
  start_utc           INTEGER NOT NULL,
  end_utc             INTEGER NOT NULL,
  start_tz            TEXT NOT NULL,
  end_tz              TEXT NOT NULL,
  is_all_day          INTEGER NOT NULL DEFAULT 0,
  recurrence          TEXT,
  recurring_event_id  TEXT,
  original_start_utc  INTEGER,
  status              TEXT NOT NULL DEFAULT 'confirmed',
  organizer_email     TEXT,
  self_response       TEXT,
  conference_uri      TEXT,
  reminders_json      TEXT,
  sequence            INTEGER NOT NULL DEFAULT 0,
  updated_at          INTEGER NOT NULL,
  UNIQUE (calendar_id, google_id)
);

-- The hot path: "give me everything overlapping this window".
CREATE INDEX idx_events_window ON events (calendar_id, start_utc, end_utc);
-- Recurring masters are fetched separately and expanded locally.
CREATE INDEX idx_events_recurring ON events (calendar_id, recurring_event_id);

CREATE TABLE attendees (
  event_id        INTEGER NOT NULL REFERENCES events(id) ON DELETE CASCADE,
  email           TEXT NOT NULL,
  display_name    TEXT,
  response_status TEXT NOT NULL,
  optional        INTEGER NOT NULL DEFAULT 0,
  is_self         INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (event_id, email)
);

CREATE TABLE sync_state (
  calendar_id        INTEGER PRIMARY KEY REFERENCES calendars(id) ON DELETE CASCADE,
  sync_token         TEXT,
  last_full_sync_at  INTEGER,
  window_start       INTEGER NOT NULL,
  window_end         INTEGER NOT NULL
);

CREATE TABLE mutations (
  id           INTEGER PRIMARY KEY,
  event_id     INTEGER REFERENCES events(id) ON DELETE CASCADE,
  kind         TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  base_etag    TEXT,
  created_at   INTEGER NOT NULL,
  attempts     INTEGER NOT NULL DEFAULT 0,
  last_error   TEXT
);

CREATE TABLE settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
```

- [ ] **Step 3: Write the failing test**

```rust
// crates/omacal-store/src/lib.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn migrations_apply_to_a_fresh_database() {
        let pool = connect_memory().await.unwrap();
        let tables: Vec<(String,)> = sqlx::query_as(
            "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        let names: Vec<String> = tables.into_iter().map(|t| t.0).collect();
        for expected in ["accounts", "attendees", "calendars", "events",
                         "mutations", "settings", "sync_state"] {
            assert!(names.contains(&expected.to_string()), "missing table {expected}");
        }
    }

    #[tokio::test]
    async fn foreign_keys_are_enforced() {
        let pool = connect_memory().await.unwrap();
        let res = sqlx::query(
            "INSERT INTO calendars (account_id, google_id, summary, timezone, access_role)
             VALUES (999, 'x', 'x', 'UTC', 'owner')",
        )
        .execute(&pool)
        .await;
        assert!(res.is_err(), "insert with a dangling account_id should fail");
    }
}
```

- [ ] **Step 4: Run test to verify it fails**

Run: `cargo test -p omacal-store`
Expected: FAIL — `cannot find function connect_memory`.

- [ ] **Step 5: Implement**

```rust
// crates/omacal-store/src/lib.rs  (above the tests module)
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

/// Opens (creating if needed) the database at `url` and runs migrations.
pub async fn connect(url: &str) -> anyhow::Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str(url)?
        .create_if_missing(true)
        .foreign_keys(true)
        // WAL keeps the UI's reads from blocking the sync task's writes.
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);
    let pool = SqlitePoolOptions::new().max_connections(5).connect_with(opts).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

/// An isolated in-memory database for tests. `max_connections(1)` is required:
/// each new connection to `:memory:` would otherwise get its own empty database.
pub async fn connect_memory() -> anyhow::Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);
    let pool = SqlitePoolOptions::new().max_connections(1).connect_with(opts).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p omacal-store`
Expected: 2 passed.

- [ ] **Step 7: Commit**

```bash
git add crates/omacal-store
git commit -m "feat(store): SQLite schema and migration runner"
```

---

### Task 6: Event upsert and window query

**Files:**
- Create: `crates/omacal-store/src/events.rs`
- Modify: `crates/omacal-store/src/lib.rs`

**Interfaces:**
- Consumes: `omacal_store::connect_memory`
- Produces:
  ```rust
  pub struct StoredEvent {
      pub id: i64, pub calendar_id: i64, pub google_id: String,
      pub summary: Option<String>, pub location: Option<String>,
      pub start_utc: i64, pub end_utc: i64,
      pub start_tz: String, pub end_tz: String,
      pub is_all_day: bool, pub recurrence: Option<String>,
      pub status: String, pub self_response: Option<String>,
      pub conference_uri: Option<String>,
  }
  pub async fn upsert_event(pool: &SqlitePool, ev: &StoredEvent) -> anyhow::Result<i64>
  pub async fn delete_event(pool: &SqlitePool, calendar_id: i64, google_id: &str) -> anyhow::Result<()>
  pub async fn events_in_window(pool: &SqlitePool, from_ms: i64, to_ms: i64)
      -> anyhow::Result<Vec<StoredEvent>>
  ```
  `events_in_window` returns non-recurring events overlapping the window **plus every recurring master on a selected calendar** regardless of window — masters are expanded by `omacal-core::expand` at render time and their `start_utc` may long predate the window.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/omacal-store/src/events.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::connect_memory;

    async fn seed(pool: &SqlitePool) -> i64 {
        sqlx::query("INSERT INTO accounts (google_sub, email, created_at) VALUES ('s','e@x',0)")
            .execute(pool).await.unwrap();
        sqlx::query(
            "INSERT INTO calendars (account_id, google_id, summary, timezone, access_role)
             VALUES (1, 'primary', 'Work', 'Europe/Sofia', 'owner')",
        ).execute(pool).await.unwrap();
        1
    }

    fn ev(cal: i64, gid: &str, start: i64, end: i64) -> StoredEvent {
        StoredEvent {
            id: 0, calendar_id: cal, google_id: gid.into(),
            summary: Some("Standup".into()), location: None,
            start_utc: start, end_utc: end,
            start_tz: "Europe/Sofia".into(), end_tz: "Europe/Sofia".into(),
            is_all_day: false, recurrence: None, status: "confirmed".into(),
            self_response: Some("accepted".into()), conference_uri: None,
        }
    }

    #[tokio::test]
    async fn an_event_round_trips() {
        let pool = connect_memory().await.unwrap();
        let cal = seed(&pool).await;
        upsert_event(&pool, &ev(cal, "a", 1000, 2000)).await.unwrap();
        let out = events_in_window(&pool, 0, 5000).await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].summary.as_deref(), Some("Standup"));
        assert_eq!(out[0].start_utc, 1000);
    }

    #[tokio::test]
    async fn upsert_updates_rather_than_duplicates() {
        let pool = connect_memory().await.unwrap();
        let cal = seed(&pool).await;
        upsert_event(&pool, &ev(cal, "a", 1000, 2000)).await.unwrap();
        let mut changed = ev(cal, "a", 1000, 2000);
        changed.summary = Some("Standup (moved)".into());
        upsert_event(&pool, &changed).await.unwrap();
        let out = events_in_window(&pool, 0, 5000).await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].summary.as_deref(), Some("Standup (moved)"));
    }

    #[tokio::test]
    async fn events_outside_the_window_are_excluded() {
        let pool = connect_memory().await.unwrap();
        let cal = seed(&pool).await;
        upsert_event(&pool, &ev(cal, "a", 10_000, 11_000)).await.unwrap();
        assert!(events_in_window(&pool, 0, 5_000).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_event_straddling_the_window_edge_is_included() {
        let pool = connect_memory().await.unwrap();
        let cal = seed(&pool).await;
        upsert_event(&pool, &ev(cal, "a", 4_000, 9_000)).await.unwrap();
        assert_eq!(events_in_window(&pool, 5_000, 6_000).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn recurring_masters_are_returned_regardless_of_window() {
        let pool = connect_memory().await.unwrap();
        let cal = seed(&pool).await;
        let mut master = ev(cal, "r", 0, 1_800_000);
        master.recurrence = Some("RRULE:FREQ=DAILY".into());
        upsert_event(&pool, &master).await.unwrap();
        // Window is far in the future; the master must still come back so the
        // caller can expand it.
        let out = events_in_window(&pool, 10_000_000_000, 10_000_100_000).await.unwrap();
        assert_eq!(out.len(), 1);
        assert!(out[0].recurrence.is_some());
    }

    #[tokio::test]
    async fn events_on_deselected_calendars_are_excluded() {
        let pool = connect_memory().await.unwrap();
        let cal = seed(&pool).await;
        upsert_event(&pool, &ev(cal, "a", 1000, 2000)).await.unwrap();
        sqlx::query("UPDATE calendars SET selected = 0").execute(&pool).await.unwrap();
        assert!(events_in_window(&pool, 0, 5000).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn deleting_an_event_removes_it() {
        let pool = connect_memory().await.unwrap();
        let cal = seed(&pool).await;
        upsert_event(&pool, &ev(cal, "a", 1000, 2000)).await.unwrap();
        delete_event(&pool, cal, "a").await.unwrap();
        assert!(events_in_window(&pool, 0, 5000).await.unwrap().is_empty());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p omacal-store events`
Expected: FAIL — `cannot find function upsert_event`.

- [ ] **Step 3: Implement**

```rust
// crates/omacal-store/src/events.rs  (above the tests module)
use sqlx::{Row, SqlitePool};

#[derive(Debug, Clone, PartialEq)]
pub struct StoredEvent {
    pub id: i64,
    pub calendar_id: i64,
    pub google_id: String,
    pub summary: Option<String>,
    pub location: Option<String>,
    pub start_utc: i64,
    pub end_utc: i64,
    /// IANA zone the start was authored in.
    pub start_tz: String,
    /// IANA zone the end was authored in. Usually equal to `start_tz`, but a
    /// flight departs in one zone and lands in another — storing both is what
    /// lets the UI later render "09:00 IST – 13:00 EET".
    pub end_tz: String,
    pub is_all_day: bool,
    pub recurrence: Option<String>,
    pub status: String,
    pub self_response: Option<String>,
    pub conference_uri: Option<String>,
}

const SELECT_COLS: &str = "e.id, e.calendar_id, e.google_id, e.summary, e.location,
     e.start_utc, e.end_utc, e.start_tz, e.end_tz, e.is_all_day, e.recurrence,
     e.status, e.self_response, e.conference_uri";

fn row_to_event(row: &sqlx::sqlite::SqliteRow) -> StoredEvent {
    StoredEvent {
        id: row.get("id"),
        calendar_id: row.get("calendar_id"),
        google_id: row.get("google_id"),
        summary: row.get("summary"),
        location: row.get("location"),
        start_utc: row.get("start_utc"),
        end_utc: row.get("end_utc"),
        start_tz: row.get("start_tz"),
        end_tz: row.get("end_tz"),
        is_all_day: row.get::<i64, _>("is_all_day") != 0,
        recurrence: row.get("recurrence"),
        status: row.get("status"),
        self_response: row.get("self_response"),
        conference_uri: row.get("conference_uri"),
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub async fn upsert_event(pool: &SqlitePool, ev: &StoredEvent) -> anyhow::Result<i64> {
    let id: i64 = sqlx::query(
        "INSERT INTO events (calendar_id, google_id, summary, location, start_utc, end_utc,
             start_tz, end_tz, is_all_day, recurrence, status, self_response,
             conference_uri, updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)
         ON CONFLICT (calendar_id, google_id) DO UPDATE SET
             summary = excluded.summary, location = excluded.location,
             start_utc = excluded.start_utc, end_utc = excluded.end_utc,
             start_tz = excluded.start_tz, end_tz = excluded.end_tz,
             is_all_day = excluded.is_all_day, recurrence = excluded.recurrence,
             status = excluded.status, self_response = excluded.self_response,
             conference_uri = excluded.conference_uri, updated_at = excluded.updated_at
         RETURNING id",
    )
    .bind(ev.calendar_id)
    .bind(&ev.google_id)
    .bind(&ev.summary)
    .bind(&ev.location)
    .bind(ev.start_utc)
    .bind(ev.end_utc)
    .bind(&ev.start_tz)
    .bind(&ev.end_tz)
    .bind(ev.is_all_day as i64)
    .bind(&ev.recurrence)
    .bind(&ev.status)
    .bind(&ev.self_response)
    .bind(&ev.conference_uri)
    .bind(now_ms())
    .fetch_one(pool)
    .await?
    .get("id");
    Ok(id)
}

pub async fn delete_event(
    pool: &SqlitePool,
    calendar_id: i64,
    google_id: &str,
) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM events WHERE calendar_id = ?1 AND google_id = ?2")
        .bind(calendar_id)
        .bind(google_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Events overlapping `[from_ms, to_ms)` on selected calendars, plus every
/// recurring master on a selected calendar. Masters are returned unconditionally
/// because their stored `start_utc` is the series start, which may be years
/// before the requested window; expansion happens in `omacal-core::expand`.
pub async fn events_in_window(
    pool: &SqlitePool,
    from_ms: i64,
    to_ms: i64,
) -> anyhow::Result<Vec<StoredEvent>> {
    let sql = format!(
        "SELECT {SELECT_COLS}
         FROM events e
         JOIN calendars c ON c.id = e.calendar_id
         WHERE c.selected = 1
           AND e.status != 'cancelled'
           AND (e.recurrence IS NOT NULL
                OR (e.start_utc < ?2 AND e.end_utc > ?1))
         ORDER BY e.start_utc"
    );
    let rows = sqlx::query(&sql).bind(from_ms).bind(to_ms).fetch_all(pool).await?;
    Ok(rows.iter().map(row_to_event).collect())
}
```

- [ ] **Step 4: Wire into the crate root**

```rust
// crates/omacal-store/src/lib.rs — add near the top
pub mod events;
pub use events::{delete_event, events_in_window, upsert_event, StoredEvent};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p omacal-store`
Expected: 9 passed.

- [ ] **Step 6: Commit**

```bash
git add crates/omacal-store
git commit -m "feat(store): event upsert, delete, and window query"
```

---

### Task 7: Google OAuth (PKCE loopback)

Implemented directly over `reqwest` rather than via the `oauth2` crate — the flow is ~100 lines, Google's endpoints are stable, and it avoids tracking that crate's builder API churn.

**Files:**
- Create: `crates/omacal-google/Cargo.toml`, `crates/omacal-google/src/lib.rs`, `crates/omacal-google/src/auth.rs`

**Interfaces:**
- Consumes: nothing
- Produces:
  ```rust
  pub struct Pkce { pub verifier: String, pub challenge: String }
  pub fn generate_pkce() -> Pkce
  pub fn authorize_url(client_id: &str, redirect_uri: &str, challenge: &str, state: &str) -> String
  pub struct Tokens { pub access_token: String, pub refresh_token: Option<String>, pub expires_at_ms: i64 }
  pub async fn exchange_code(token_endpoint: &str, client_id: &str, client_secret: &str,
                             code: &str, verifier: &str, redirect_uri: &str) -> anyhow::Result<Tokens>
  pub async fn refresh(token_endpoint: &str, client_id: &str, client_secret: &str,
                       refresh_token: &str) -> anyhow::Result<Tokens>
  ```
  `token_endpoint` is a parameter, not a constant, so tests can point it at `wiremock`.

- [ ] **Step 1: Create the crate**

```bash
cargo new --lib crates/omacal-google
cargo add --package omacal-google reqwest --features json,rustls-tls --no-default-features
cargo add --package omacal-google serde serde_json anyhow thiserror base64 sha2 rand url
cargo add --package omacal-google --dev wiremock tokio --features tokio/rt-multi-thread,tokio/macros
```

- [ ] **Step 2: Write the failing tests**

```rust
// crates/omacal-google/src/auth.rs
#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn pkce_challenge_is_url_safe_and_unpadded() {
        let p = generate_pkce();
        assert!(p.challenge.len() >= 43);
        assert!(!p.challenge.contains('='), "challenge must be unpadded");
        assert!(!p.challenge.contains('+') && !p.challenge.contains('/'),
                "challenge must be URL-safe base64");
    }

    #[test]
    fn pkce_verifiers_differ_between_calls() {
        assert_ne!(generate_pkce().verifier, generate_pkce().verifier);
    }

    #[test]
    fn the_authorize_url_carries_everything_google_requires() {
        let url = authorize_url("cid", "http://127.0.0.1:9999", "chal", "st");
        assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
        assert!(url.contains("client_id=cid"));
        assert!(url.contains("code_challenge=chal"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=st"));
        // Both are required to receive a refresh token at all.
        assert!(url.contains("access_type=offline"));
        assert!(url.contains("prompt=consent"));
        assert!(url.contains("calendar"));
    }

    #[tokio::test]
    async fn exchanging_a_code_returns_tokens() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "at-1",
                "refresh_token": "rt-1",
                "expires_in": 3599,
                "token_type": "Bearer"
            })))
            .mount(&server)
            .await;

        let t = exchange_code(&format!("{}/token", server.uri()),
                              "cid", "secret", "code", "verifier", "http://127.0.0.1:9999")
            .await.unwrap();
        assert_eq!(t.access_token, "at-1");
        assert_eq!(t.refresh_token.as_deref(), Some("rt-1"));
        assert!(t.expires_at_ms > 0);
    }

    #[tokio::test]
    async fn a_refresh_response_without_a_refresh_token_is_accepted() {
        // Google omits refresh_token on refresh; the caller keeps the old one.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "at-2", "expires_in": 3599, "token_type": "Bearer"
            })))
            .mount(&server)
            .await;

        let t = refresh(&format!("{}/token", server.uri()), "cid", "secret", "rt-1")
            .await.unwrap();
        assert_eq!(t.access_token, "at-2");
        assert!(t.refresh_token.is_none());
    }

    #[tokio::test]
    async fn an_oauth_error_response_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": "invalid_grant",
                "error_description": "Token has been expired or revoked."
            })))
            .mount(&server)
            .await;

        let err = refresh(&format!("{}/token", server.uri()), "cid", "secret", "rt-old")
            .await.unwrap_err();
        assert!(err.to_string().contains("invalid_grant"));
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p omacal-google`
Expected: FAIL — `cannot find function generate_pkce`.

- [ ] **Step 4: Implement**

```rust
// crates/omacal-google/src/auth.rs  (above the tests module)
use anyhow::{bail, Context};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use sha2::{Digest, Sha256};

pub const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
pub const SCOPE: &str = "https://www.googleapis.com/auth/calendar";

pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

/// RFC 7636 S256 pair. The verifier is 32 random bytes, base64url-unpadded.
pub fn generate_pkce() -> Pkce {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let verifier = URL_SAFE_NO_PAD.encode(bytes);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    Pkce { verifier, challenge }
}

/// `access_type=offline` plus `prompt=consent` is what makes Google issue a
/// refresh token. Without both, re-authorising an already-consented account
/// silently returns an access token only.
pub fn authorize_url(client_id: &str, redirect_uri: &str, challenge: &str, state: &str) -> String {
    let q = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", SCOPE)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state)
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent")
        .finish();
    format!("{AUTH_ENDPOINT}?{q}")
}

#[derive(Debug, Clone)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at_ms: i64,
}

#[derive(serde::Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    error: Option<String>,
    error_description: Option<String>,
}

async fn post_token(endpoint: &str, form: &[(&str, &str)]) -> anyhow::Result<Tokens> {
    let resp = reqwest::Client::new()
        .post(endpoint)
        .form(form)
        .send()
        .await
        .context("token endpoint unreachable")?;

    let body: TokenResponse = resp.json().await.context("token response was not JSON")?;

    if let Some(err) = body.error {
        bail!("{}: {}", err, body.error_description.unwrap_or_default());
    }

    let access_token = body.access_token.context("token response had no access_token")?;
    let expires_in = body.expires_in.unwrap_or(3600);
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as i64;

    Ok(Tokens {
        access_token,
        refresh_token: body.refresh_token,
        // 60s of slack so a request in flight does not expire mid-call.
        expires_at_ms: now_ms + (expires_in - 60).max(0) * 1000,
    })
}

pub async fn exchange_code(
    token_endpoint: &str,
    client_id: &str,
    client_secret: &str,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> anyhow::Result<Tokens> {
    post_token(
        token_endpoint,
        &[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("code", code),
            ("code_verifier", verifier),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect_uri),
        ],
    )
    .await
}

pub async fn refresh(
    token_endpoint: &str,
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
) -> anyhow::Result<Tokens> {
    post_token(
        token_endpoint,
        &[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ],
    )
    .await
}
```

```rust
// crates/omacal-google/src/lib.rs
pub mod auth;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p omacal-google`
Expected: 6 passed.

If `rand::rng()` does not resolve, the installed `rand` is 0.8 — use `rand::thread_rng()` instead.

- [ ] **Step 6: Write the failing test for the loopback listener**

```rust
// crates/omacal-google/src/auth.rs — add to the tests module
#[tokio::test]
async fn the_loopback_listener_captures_code_and_state() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let handle = tokio::task::spawn_blocking(move || wait_for_redirect(listener));

    // Simulate the browser hitting the redirect URI.
    let _ = reqwest::get(format!("http://127.0.0.1:{port}/?code=abc123&state=xyz")).await;

    let got = handle.await.unwrap().unwrap();
    assert_eq!(got.code, "abc123");
    assert_eq!(got.state, "xyz");
}

#[tokio::test]
async fn the_loopback_listener_reports_a_denied_consent() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = tokio::task::spawn_blocking(move || wait_for_redirect(listener));
    let _ = reqwest::get(format!("http://127.0.0.1:{port}/?error=access_denied")).await;
    assert!(handle.await.unwrap().is_err());
}
```

- [ ] **Step 7: Implement the loopback listener**

```rust
// crates/omacal-google/src/auth.rs  (above the tests module)
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;

pub struct Redirect {
    pub code: String,
    pub state: String,
}

/// Binds an ephemeral loopback port for the OAuth redirect.
///
/// Google's installed-app flow allows any `http://127.0.0.1:<port>` redirect
/// without pre-registering the port, so we take whatever the OS gives us.
pub fn bind_loopback() -> anyhow::Result<(TcpListener, String)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    Ok((listener, format!("http://127.0.0.1:{port}")))
}

/// Blocks until the browser hits the redirect URI, then returns the code.
///
/// Blocking is deliberate: call it from `spawn_blocking`. Writing an async
/// HTTP server for a single one-shot request would be more machinery than the
/// problem deserves.
pub fn wait_for_redirect(listener: TcpListener) -> anyhow::Result<Redirect> {
    let (mut stream, _) = listener.accept()?;
    let mut line = String::new();
    BufReader::new(stream.try_clone()?).read_line(&mut line)?;

    // "GET /?code=...&state=... HTTP/1.1"
    let target = line.split_whitespace().nth(1).unwrap_or("/");
    let url = url::Url::parse(&format!("http://127.0.0.1{target}"))?;
    let params: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();

    let body = if params.contains_key("code") {
        "<html><body style=\"font:14px system-ui;padding:3rem\">\
         Signed in. You can close this tab.</body></html>"
    } else {
        "<html><body style=\"font:14px system-ui;padding:3rem\">\
         Sign-in failed. You can close this tab.</body></html>"
    };
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    )?;
    stream.flush()?;

    match (params.get("code"), params.get("state")) {
        (Some(code), Some(state)) => Ok(Redirect { code: code.clone(), state: state.clone() }),
        _ => anyhow::bail!(
            "authorisation failed: {}",
            params.get("error").map(String::as_str).unwrap_or("no code returned")
        ),
    }
}
```

- [ ] **Step 8: Run tests to verify they pass**

Run: `cargo test -p omacal-google`
Expected: 8 passed.

- [ ] **Step 9: Commit**

```bash
git add crates/omacal-google
git commit -m "feat(google): PKCE OAuth exchange, refresh, and loopback listener"
```

---

### Task 8: Calendar API client

**Files:**
- Create: `crates/omacal-google/src/model.rs`, `crates/omacal-google/src/client.rs`
- Modify: `crates/omacal-google/src/lib.rs`

**Interfaces:**
- Consumes: `auth::Tokens`
- Produces:
  ```rust
  pub struct CalendarClient { /* base_url + access token */ }
  impl CalendarClient {
      pub fn new(base_url: impl Into<String>, access_token: impl Into<String>) -> Self;
      pub async fn list_calendars(&self) -> anyhow::Result<Vec<model::Calendar>>;
      pub async fn list_events(&self, calendar_id: &str, req: &EventsRequest)
          -> Result<EventsPage, ApiError>;
  }
  pub struct EventsRequest { pub sync_token: Option<String>, pub time_min: Option<String>,
                             pub time_max: Option<String>, pub page_token: Option<String> }
  pub struct EventsPage { pub events: Vec<model::Event>, pub next_page_token: Option<String>,
                          pub next_sync_token: Option<String> }
  pub enum ApiError { SyncTokenInvalid, Http(String), Transport(String) }
  ```
  `ApiError::SyncTokenInvalid` is the typed `410 GONE` that Task 9 recovers from.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/omacal-google/src/client.rs
#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn list_calendars_parses_the_payload() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/users/me/calendarList"))
            .and(header("authorization", "Bearer at-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [{
                    "id": "primary", "summary": "Work", "backgroundColor": "#5b8def",
                    "timeZone": "Europe/Sofia", "accessRole": "owner", "primary": true
                }]
            })))
            .mount(&server).await;

        let c = CalendarClient::new(server.uri(), "at-1");
        let cals = c.list_calendars().await.unwrap();
        assert_eq!(cals.len(), 1);
        assert_eq!(cals[0].id, "primary");
        assert_eq!(cals[0].time_zone.as_deref(), Some("Europe/Sofia"));
        assert!(cals[0].primary);
    }

    #[tokio::test]
    async fn a_full_sync_sends_single_events_false_and_returns_a_sync_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/calendars/primary/events"))
            .and(query_param("singleEvents", "false"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [{
                    "id": "e1", "status": "confirmed", "summary": "Standup",
                    "start": {"dateTime": "2026-08-03T09:00:00+03:00", "timeZone": "Europe/Sofia"},
                    "end":   {"dateTime": "2026-08-03T09:30:00+03:00", "timeZone": "Europe/Sofia"},
                    "recurrence": ["RRULE:FREQ=DAILY"]
                }],
                "nextSyncToken": "tok-1"
            })))
            .mount(&server).await;

        let c = CalendarClient::new(server.uri(), "at-1");
        let page = c.list_events("primary", &EventsRequest::default()).await.unwrap();
        assert_eq!(page.events.len(), 1);
        assert_eq!(page.next_sync_token.as_deref(), Some("tok-1"));
        assert_eq!(page.events[0].recurrence.as_ref().unwrap()[0], "RRULE:FREQ=DAILY");
    }

    #[tokio::test]
    async fn an_all_day_event_parses_its_date_form() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/calendars/primary/events"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [{
                    "id": "e2", "status": "confirmed", "summary": "Sofia trip",
                    "start": {"date": "2026-08-08"},
                    "end":   {"date": "2026-08-17"}
                }],
                "nextSyncToken": "tok-2"
            })))
            .mount(&server).await;

        let c = CalendarClient::new(server.uri(), "at-1");
        let page = c.list_events("primary", &EventsRequest::default()).await.unwrap();
        assert_eq!(page.events[0].start.date.as_deref(), Some("2026-08-08"));
        assert!(page.events[0].start.date_time.is_none());
    }

    #[tokio::test]
    async fn a_cancelled_instance_is_returned_not_dropped() {
        // Incremental syncs deliver deletions as status=cancelled tombstones.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/calendars/primary/events"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [{"id": "e3", "status": "cancelled"}],
                "nextSyncToken": "tok-3"
            })))
            .mount(&server).await;

        let c = CalendarClient::new(server.uri(), "at-1");
        let page = c.list_events("primary", &EventsRequest::default()).await.unwrap();
        assert_eq!(page.events[0].status, "cancelled");
    }

    #[tokio::test]
    async fn a_410_becomes_sync_token_invalid() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/calendars/primary/events"))
            .respond_with(ResponseTemplate::new(410).set_body_json(serde_json::json!({
                "error": {"code": 410, "message": "Sync token is no longer valid"}
            })))
            .mount(&server).await;

        let c = CalendarClient::new(server.uri(), "at-1");
        let req = EventsRequest { sync_token: Some("stale".into()), ..Default::default() };
        match c.list_events("primary", &req).await {
            Err(ApiError::SyncTokenInvalid) => {}
            other => panic!("expected SyncTokenInvalid, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_500_is_a_plain_http_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/calendars/primary/events"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server).await;

        let c = CalendarClient::new(server.uri(), "at-1");
        match c.list_events("primary", &EventsRequest::default()).await {
            Err(ApiError::Http(_)) => {}
            other => panic!("expected Http, got {other:?}"),
        }
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p omacal-google client`
Expected: FAIL — `cannot find type CalendarClient`.

- [ ] **Step 3: Implement the wire model**

```rust
// crates/omacal-google/src/model.rs
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Calendar {
    pub id: String,
    #[serde(default)]
    pub summary: String,
    pub background_color: Option<String>,
    pub time_zone: Option<String>,
    #[serde(default)]
    pub access_role: String,
    #[serde(default)]
    pub primary: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventDateTime {
    /// Present for timed events, RFC 3339.
    pub date_time: Option<String>,
    /// Present for all-day events, `YYYY-MM-DD`. The `end` date is exclusive.
    pub date: Option<String>,
    pub time_zone: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attendee {
    #[serde(default)]
    pub email: String,
    pub display_name: Option<String>,
    #[serde(default)]
    pub response_status: String,
    #[serde(default)]
    pub optional: bool,
    #[serde(rename = "self", default)]
    pub is_self: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: String,
    /// `confirmed` | `tentative` | `cancelled`. Cancelled rows are tombstones
    /// delivered by incremental sync and carry almost no other fields.
    #[serde(default)]
    pub status: String,
    pub etag: Option<String>,
    #[serde(rename = "iCalUID")]
    pub ical_uid: Option<String>,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub location: Option<String>,
    #[serde(default)]
    pub start: EventDateTime,
    #[serde(default)]
    pub end: EventDateTime,
    pub recurrence: Option<Vec<String>>,
    pub recurring_event_id: Option<String>,
    pub original_start_time: Option<EventDateTime>,
    pub hangout_link: Option<String>,
    #[serde(default)]
    pub attendees: Vec<Attendee>,
    #[serde(default)]
    pub sequence: i64,
}
```

- [ ] **Step 4: Implement the client**

```rust
// crates/omacal-google/src/client.rs  (above the tests module)
use crate::model;
use serde::Deserialize;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// HTTP 410 — the stored sync token is stale. The caller must discard it
    /// and perform a full resync (spec §5).
    #[error("sync token is no longer valid")]
    SyncTokenInvalid,
    #[error("http error: {0}")]
    Http(String),
    #[error("transport error: {0}")]
    Transport(String),
}

#[derive(Debug, Clone, Default)]
pub struct EventsRequest {
    pub sync_token: Option<String>,
    pub time_min: Option<String>,
    pub time_max: Option<String>,
    pub page_token: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EventsPage {
    pub events: Vec<model::Event>,
    pub next_page_token: Option<String>,
    pub next_sync_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EventsResponse {
    #[serde(default)]
    items: Vec<model::Event>,
    next_page_token: Option<String>,
    next_sync_token: Option<String>,
}

#[derive(Deserialize)]
struct CalendarListResponse {
    #[serde(default)]
    items: Vec<model::Calendar>,
}

pub struct CalendarClient {
    base_url: String,
    access_token: String,
    http: reqwest::Client,
}

impl CalendarClient {
    /// `base_url` is `https://www.googleapis.com/calendar/v3` in production and
    /// a `wiremock` URI in tests.
    pub fn new(base_url: impl Into<String>, access_token: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            access_token: access_token.into(),
            http: reqwest::Client::new(),
        }
    }

    pub async fn list_calendars(&self) -> anyhow::Result<Vec<model::Calendar>> {
        let resp = self
            .http
            .get(format!("{}/users/me/calendarList", self.base_url))
            .bearer_auth(&self.access_token)
            .send()
            .await?
            .error_for_status()?
            .json::<CalendarListResponse>()
            .await?;
        Ok(resp.items)
    }

    /// One page of events.
    ///
    /// `singleEvents=false` is deliberate (spec §5): we store recurring masters
    /// and expand locally. Every parameter here must stay byte-identical across
    /// incremental calls or Google invalidates the sync token.
    pub async fn list_events(
        &self,
        calendar_id: &str,
        req: &EventsRequest,
    ) -> Result<EventsPage, ApiError> {
        let mut params: Vec<(&str, String)> = vec![
            ("singleEvents", "false".into()),
            ("showDeleted", "true".into()),
            ("maxResults", "2500".into()),
        ];
        // timeMin/timeMax are illegal alongside a syncToken.
        if let Some(t) = &req.sync_token {
            params.push(("syncToken", t.clone()));
        } else {
            if let Some(t) = &req.time_min {
                params.push(("timeMin", t.clone()));
            }
            if let Some(t) = &req.time_max {
                params.push(("timeMax", t.clone()));
            }
        }
        if let Some(t) = &req.page_token {
            params.push(("pageToken", t.clone()));
        }

        let resp = self
            .http
            .get(format!(
                "{}/calendars/{}/events",
                self.base_url,
                urlencoding_path(calendar_id)
            ))
            .bearer_auth(&self.access_token)
            .query(&params)
            .send()
            .await
            .map_err(|e| ApiError::Transport(e.to_string()))?;

        if resp.status() == reqwest::StatusCode::GONE {
            return Err(ApiError::SyncTokenInvalid);
        }
        if !resp.status().is_success() {
            return Err(ApiError::Http(format!("{}", resp.status())));
        }

        let body: EventsResponse = resp
            .json()
            .await
            .map_err(|e| ApiError::Transport(e.to_string()))?;

        Ok(EventsPage {
            events: body.items,
            next_page_token: body.next_page_token,
            next_sync_token: body.next_sync_token,
        })
    }
}

/// Calendar ids are email-like and must be percent-encoded in the path.
fn urlencoding_path(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}
```

```rust
// crates/omacal-google/src/lib.rs
pub mod auth;
pub mod client;
pub mod model;

pub use client::{ApiError, CalendarClient, EventsPage, EventsRequest};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p omacal-google`
Expected: 14 passed (8 existing auth tests + 6 new client tests).

- [ ] **Step 6: Commit**

```bash
git add crates/omacal-google
git commit -m "feat(google): calendar list and incremental events client"
```

---

### Task 9: Sync orchestration

**Files:**
- Create: `crates/omacal-sync/Cargo.toml`, `crates/omacal-sync/src/lib.rs`, `crates/omacal-sync/src/convert.rs`

**Interfaces:**
- Consumes: `omacal_google::{CalendarClient, EventsRequest, ApiError, model}`, `omacal_store::{StoredEvent, upsert_event, delete_event}`
- Produces:
  ```rust
  pub fn to_stored(ev: &model::Event, calendar_id: i64, cal_tz: &str)
      -> Option<StoredEvent>              // None for tombstones
  pub fn is_tombstone(ev: &model::Event) -> bool
  pub async fn sync_calendar(pool: &SqlitePool, client: &CalendarClient,
                             calendar_id: i64, google_id: &str,
                             window_start_ms: i64, window_end_ms: i64) -> anyhow::Result<SyncOutcome>
  pub struct SyncOutcome { pub upserted: usize, pub deleted: usize, pub did_full_resync: bool }
  ```

- [ ] **Step 1: Create the crate**

```bash
cargo new --lib crates/omacal-sync
cargo add --package omacal-sync --path crates/omacal-core
cargo add --package omacal-sync --path crates/omacal-store
cargo add --package omacal-sync --path crates/omacal-google
cargo add --package omacal-sync anyhow jiff tracing sqlx --features sqlx/runtime-tokio,sqlx/sqlite
cargo add --package omacal-sync --dev serde_json
cargo add --package omacal-sync --dev wiremock tokio --features tokio/rt-multi-thread,tokio/macros
```

- [ ] **Step 2: Write the failing conversion tests**

```rust
// crates/omacal-sync/src/convert.rs
#[cfg(test)]
mod tests {
    use super::*;
    use omacal_google::model::{Event, EventDateTime};

    fn timed(start: &str, end: &str) -> Event {
        Event {
            id: "e1".into(), status: "confirmed".into(), etag: None, ical_uid: None,
            summary: Some("Standup".into()), description: None, location: Some("Meet".into()),
            start: EventDateTime { date_time: Some(start.into()), date: None,
                                   time_zone: Some("Europe/Sofia".into()) },
            end: EventDateTime { date_time: Some(end.into()), date: None,
                                 time_zone: Some("Europe/Sofia".into()) },
            recurrence: None, recurring_event_id: None, original_start_time: None,
            hangout_link: None, attendees: vec![], sequence: 0,
        }
    }

    #[test]
    fn a_timed_event_converts_to_utc_millis() {
        let ev = timed("2026-08-03T09:00:00+03:00", "2026-08-03T09:30:00+03:00");
        let s = to_stored(&ev, 1, "Europe/Sofia").unwrap();
        // 2026-08-03T09:00:00+03:00 == 2026-08-03T06:00:00Z
        assert_eq!(s.start_utc, 1_785_736_800_000);
        assert_eq!(s.end_utc - s.start_utc, 30 * 60_000);
        assert_eq!(s.start_tz, "Europe/Sofia");
        assert!(!s.is_all_day);
    }

    /// A flight departs in one zone and lands in another. Both must survive.
    #[test]
    fn a_cross_timezone_event_keeps_both_zones() {
        let mut ev = timed("2026-08-09T09:00:00+05:30", "2026-08-09T13:00:00+03:00");
        ev.start.time_zone = Some("Asia/Kolkata".into());
        ev.end.time_zone = Some("Europe/Sofia".into());
        let s = to_stored(&ev, 1, "Europe/Sofia").unwrap();
        assert_eq!(s.start_tz, "Asia/Kolkata");
        assert_eq!(s.end_tz, "Europe/Sofia");
        // 09:00 IST is 03:30Z; 13:00 EEST is 10:00Z.
        assert_eq!(s.end_utc - s.start_utc, 6 * 3_600_000 + 1_800_000);
    }

    #[test]
    fn end_zone_defaults_to_the_start_zone_when_absent() {
        let mut ev = timed("2026-08-03T09:00:00+03:00", "2026-08-03T09:30:00+03:00");
        ev.end.time_zone = None;
        let s = to_stored(&ev, 1, "UTC").unwrap();
        assert_eq!(s.end_tz, "Europe/Sofia");
    }

    #[test]
    fn an_all_day_event_uses_the_calendar_timezone() {
        let mut ev = timed("", "");
        ev.start = EventDateTime { date: Some("2026-08-08".into()), ..Default::default() };
        ev.end = EventDateTime { date: Some("2026-08-09".into()), ..Default::default() };
        let s = to_stored(&ev, 1, "Europe/Sofia").unwrap();
        assert!(s.is_all_day);
        // Google's all-day end date is exclusive; one calendar day must remain
        // exactly one day long.
        assert_eq!(s.end_utc - s.start_utc, 24 * 3_600_000);
    }

    #[test]
    fn a_cancelled_event_is_a_tombstone() {
        let mut ev = timed("2026-08-03T09:00:00+03:00", "2026-08-03T09:30:00+03:00");
        ev.status = "cancelled".into();
        assert!(is_tombstone(&ev));
        assert!(to_stored(&ev, 1, "Europe/Sofia").is_none());
    }

    #[test]
    fn recurrence_lines_are_joined_with_newlines() {
        let mut ev = timed("2026-08-03T09:00:00+03:00", "2026-08-03T09:30:00+03:00");
        ev.recurrence = Some(vec!["RRULE:FREQ=DAILY".into(), "EXDATE:20260804T060000Z".into()]);
        let s = to_stored(&ev, 1, "Europe/Sofia").unwrap();
        assert_eq!(s.recurrence.unwrap(), "RRULE:FREQ=DAILY\nEXDATE:20260804T060000Z");
    }

    #[test]
    fn the_self_attendee_response_is_captured() {
        let mut ev = timed("2026-08-03T09:00:00+03:00", "2026-08-03T09:30:00+03:00");
        ev.attendees = vec![
            omacal_google::model::Attendee {
                email: "other@x".into(), display_name: None,
                response_status: "accepted".into(), optional: false, is_self: false },
            omacal_google::model::Attendee {
                email: "me@x".into(), display_name: None,
                response_status: "needsAction".into(), optional: false, is_self: true },
        ];
        let s = to_stored(&ev, 1, "Europe/Sofia").unwrap();
        assert_eq!(s.self_response.as_deref(), Some("needsAction"));
    }

    #[test]
    fn an_unparseable_start_is_skipped_rather_than_panicking() {
        let ev = timed("not-a-date", "also-not-a-date");
        assert!(to_stored(&ev, 1, "Europe/Sofia").is_none());
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p omacal-sync`
Expected: FAIL — `cannot find function to_stored`.

- [ ] **Step 4: Implement conversion**

```rust
// crates/omacal-sync/src/convert.rs  (above the tests module)
use jiff::civil::Date;
use jiff::Timestamp;
use omacal_google::model::{Event, EventDateTime};
use omacal_store::StoredEvent;

/// Incremental sync delivers deletions as `status: "cancelled"` rows that carry
/// little more than an id.
pub fn is_tombstone(ev: &Event) -> bool {
    ev.status == "cancelled"
}

/// Resolves one endpoint to an epoch-millisecond instant.
///
/// Timed events carry RFC 3339 with an offset. All-day events carry a bare
/// date, which must be interpreted in the calendar's zone — midnight in Sofia
/// is not midnight UTC.
fn resolve(dt: &EventDateTime, cal_tz: &str) -> Option<i64> {
    if let Some(s) = &dt.date_time {
        return s.parse::<Timestamp>().ok().map(|t| t.as_millisecond());
    }
    let d = dt.date.as_ref()?;
    let date: Date = d.parse().ok()?;
    let tz = dt.time_zone.as_deref().unwrap_or(cal_tz);
    date.to_datetime(jiff::civil::Time::midnight())
        .in_tz(tz)
        .ok()
        .map(|z| z.timestamp().as_millisecond())
}

/// Converts a wire event into a storable row. Returns `None` for tombstones and
/// for rows whose times cannot be parsed — a malformed event must not abort a
/// whole sync page.
pub fn to_stored(ev: &Event, calendar_id: i64, cal_tz: &str) -> Option<StoredEvent> {
    if is_tombstone(ev) {
        return None;
    }
    let start_utc = resolve(&ev.start, cal_tz)?;
    let end_utc = resolve(&ev.end, cal_tz)?;
    let is_all_day = ev.start.date.is_some();

    Some(StoredEvent {
        id: 0,
        calendar_id,
        google_id: ev.id.clone(),
        summary: ev.summary.clone(),
        location: ev.location.clone(),
        start_utc,
        end_utc,
        start_tz: ev
            .start
            .time_zone
            .clone()
            .unwrap_or_else(|| cal_tz.to_string()),
        // Kept separately from `start_tz`: a flight departs in one zone and
        // lands in another, and collapsing the two loses that.
        end_tz: ev
            .end
            .time_zone
            .clone()
            .or_else(|| ev.start.time_zone.clone())
            .unwrap_or_else(|| cal_tz.to_string()),
        is_all_day,
        recurrence: ev.recurrence.as_ref().map(|r| r.join("\n")),
        status: ev.status.clone(),
        self_response: ev
            .attendees
            .iter()
            .find(|a| a.is_self)
            .map(|a| a.response_status.clone()),
        conference_uri: ev.hangout_link.clone(),
    })
}
```

- [ ] **Step 5: Run conversion tests**

Run: `cargo test -p omacal-sync convert`
Expected: 6 passed.

- [ ] **Step 6: Write the failing orchestration test**

```rust
// crates/omacal-sync/src/lib.rs
#[cfg(test)]
mod tests {
    use super::*;
    use omacal_google::CalendarClient;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn seeded_pool() -> sqlx::SqlitePool {
        let pool = omacal_store::connect_memory().await.unwrap();
        sqlx::query("INSERT INTO accounts (google_sub, email, created_at) VALUES ('s','e@x',0)")
            .execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO calendars (account_id, google_id, summary, timezone, access_role)
             VALUES (1, 'primary', 'Work', 'Europe/Sofia', 'owner')",
        ).execute(&pool).await.unwrap();
        pool
    }

    fn one_event_body(token: &str) -> serde_json::Value {
        serde_json::json!({
            "items": [{
                "id": "e1", "status": "confirmed", "summary": "Standup",
                "start": {"dateTime": "2026-08-03T09:00:00+03:00", "timeZone": "Europe/Sofia"},
                "end":   {"dateTime": "2026-08-03T09:30:00+03:00", "timeZone": "Europe/Sofia"}
            }],
            "nextSyncToken": token
        })
    }

    #[tokio::test]
    async fn a_first_sync_stores_events_and_records_the_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/calendars/primary/events"))
            .respond_with(ResponseTemplate::new(200).set_body_json(one_event_body("tok-1")))
            .mount(&server).await;

        let pool = seeded_pool().await;
        let client = CalendarClient::new(server.uri(), "at-1");
        let out = sync_calendar(&pool, &client, 1, "primary", 0, 9_999_999_999_999).await.unwrap();

        assert_eq!(out.upserted, 1);
        assert!(!out.did_full_resync);
        let tok: Option<String> = sqlx::query_scalar(
            "SELECT sync_token FROM sync_state WHERE calendar_id = 1")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(tok.as_deref(), Some("tok-1"));
    }

    #[tokio::test]
    async fn a_tombstone_deletes_the_local_row() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/calendars/primary/events"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [{"id": "e1", "status": "cancelled"}],
                "nextSyncToken": "tok-2"
            })))
            .mount(&server).await;

        let pool = seeded_pool().await;
        omacal_store::upsert_event(&pool, &omacal_store::StoredEvent {
            id: 0, calendar_id: 1, google_id: "e1".into(), summary: Some("Standup".into()),
            location: None, start_utc: 1000, end_utc: 2000,
            start_tz: "Europe/Sofia".into(), end_tz: "Europe/Sofia".into(),
            is_all_day: false, recurrence: None, status: "confirmed".into(),
            self_response: None, conference_uri: None,
        }).await.unwrap();

        let client = CalendarClient::new(server.uri(), "at-1");
        let out = sync_calendar(&pool, &client, 1, "primary", 0, 9_999_999_999_999).await.unwrap();
        assert_eq!(out.deleted, 1);
        assert!(omacal_store::events_in_window(&pool, 0, 5000).await.unwrap().is_empty());
    }

    /// The recovery path from spec §5. A stale token must not be fatal.
    #[tokio::test]
    async fn a_410_triggers_a_full_resync() {
        let server = MockServer::start().await;
        // With a syncToken: 410.
        Mock::given(method("GET")).and(path("/calendars/primary/events"))
            .and(query_param("syncToken", "stale"))
            .respond_with(ResponseTemplate::new(410))
            .mount(&server).await;
        // Without one: succeed.
        Mock::given(method("GET")).and(path("/calendars/primary/events"))
            .and(query_param("singleEvents", "false"))
            .respond_with(ResponseTemplate::new(200).set_body_json(one_event_body("tok-fresh")))
            .mount(&server).await;

        let pool = seeded_pool().await;
        sqlx::query(
            "INSERT INTO sync_state (calendar_id, sync_token, window_start, window_end)
             VALUES (1, 'stale', 0, 0)")
            .execute(&pool).await.unwrap();

        let client = CalendarClient::new(server.uri(), "at-1");
        let out = sync_calendar(&pool, &client, 1, "primary", 0, 9_999_999_999_999).await.unwrap();

        assert!(out.did_full_resync);
        assert_eq!(out.upserted, 1);
        let tok: Option<String> = sqlx::query_scalar(
            "SELECT sync_token FROM sync_state WHERE calendar_id = 1")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(tok.as_deref(), Some("tok-fresh"));
    }
}
```

- [ ] **Step 7: Run to verify it fails**

Run: `cargo test -p omacal-sync sync_calendar`
Expected: FAIL — `cannot find function sync_calendar`.

- [ ] **Step 8: Implement orchestration**

```rust
// crates/omacal-sync/src/lib.rs  (above the tests module)
pub mod convert;
pub use convert::{is_tombstone, to_stored};

use omacal_google::{ApiError, CalendarClient, EventsRequest};
use sqlx::SqlitePool;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct SyncOutcome {
    pub upserted: usize,
    pub deleted: usize,
    pub did_full_resync: bool,
}

fn to_rfc3339(ms: i64) -> String {
    jiff::Timestamp::from_millisecond(ms)
        .map(|t| t.to_string())
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

/// Syncs one calendar, following pagination and recovering from a stale token.
///
/// Uses the stored `sync_token` when present. On `410 GONE` the token is
/// discarded and a full windowed sync runs instead — expected behaviour, not an
/// error (spec §5).
pub async fn sync_calendar(
    pool: &SqlitePool,
    client: &CalendarClient,
    calendar_id: i64,
    google_id: &str,
    window_start_ms: i64,
    window_end_ms: i64,
) -> anyhow::Result<SyncOutcome> {
    let cal_tz: String =
        sqlx::query_scalar("SELECT timezone FROM calendars WHERE id = ?1")
            .bind(calendar_id)
            .fetch_one(pool)
            .await?;

    let stored_token: Option<String> =
        sqlx::query_scalar("SELECT sync_token FROM sync_state WHERE calendar_id = ?1")
            .bind(calendar_id)
            .fetch_optional(pool)
            .await?
            .flatten();

    let mut outcome = SyncOutcome::default();
    let mut token = stored_token;

    loop {
        match drain(pool, client, calendar_id, google_id, &cal_tz,
                    token.clone(), window_start_ms, window_end_ms, &mut outcome).await
        {
            Ok(next) => {
                sqlx::query(
                    "INSERT INTO sync_state (calendar_id, sync_token, window_start, window_end)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT (calendar_id) DO UPDATE SET
                         sync_token = excluded.sync_token,
                         window_start = excluded.window_start,
                         window_end = excluded.window_end",
                )
                .bind(calendar_id)
                .bind(&next)
                .bind(window_start_ms)
                .bind(window_end_ms)
                .execute(pool)
                .await?;
                return Ok(outcome);
            }
            Err(ApiError::SyncTokenInvalid) if token.is_some() => {
                tracing::warn!(calendar_id, "sync token rejected, falling back to full resync");
                outcome = SyncOutcome { did_full_resync: true, ..Default::default() };
                token = None;
                continue;
            }
            Err(e) => return Err(e.into()),
        }
    }
}

/// Walks every page for one attempt, returning the final `nextSyncToken`.
#[allow(clippy::too_many_arguments)]
async fn drain(
    pool: &SqlitePool,
    client: &CalendarClient,
    calendar_id: i64,
    google_id: &str,
    cal_tz: &str,
    sync_token: Option<String>,
    window_start_ms: i64,
    window_end_ms: i64,
    outcome: &mut SyncOutcome,
) -> Result<Option<String>, ApiError> {
    let mut page_token: Option<String> = None;

    loop {
        let req = EventsRequest {
            sync_token: sync_token.clone(),
            time_min: sync_token.is_none().then(|| to_rfc3339(window_start_ms)),
            time_max: sync_token.is_none().then(|| to_rfc3339(window_end_ms)),
            page_token: page_token.clone(),
        };
        let page = client.list_events(google_id, &req).await?;

        for ev in &page.events {
            if is_tombstone(ev) {
                if omacal_store::delete_event(pool, calendar_id, &ev.id).await.is_ok() {
                    outcome.deleted += 1;
                }
            } else if let Some(stored) = to_stored(ev, calendar_id, cal_tz) {
                if omacal_store::upsert_event(pool, &stored).await.is_ok() {
                    outcome.upserted += 1;
                }
            } else {
                tracing::warn!(event_id = %ev.id, "skipping unparseable event");
            }
        }

        match page.next_page_token {
            Some(t) => page_token = Some(t),
            None => return Ok(page.next_sync_token),
        }
    }
}
```

Add `tracing` to the crate:

```bash
cargo add --package omacal-sync tracing
```

- [ ] **Step 9: Run tests to verify they pass**

Run: `cargo test -p omacal-sync`
Expected: 9 passed.

- [ ] **Step 10: Commit**

```bash
git add crates/omacal-sync
git commit -m "feat(sync): incremental calendar sync with 410 resync recovery"
```

---

### Task 10: Theme resolution

Implements spec §10, including the fallback chain that guarantees the app starts even with an unreadable theme.

**Files:**
- Create: `src-tauri/src/theme.rs`
- Create: `src-tauri/tests/fixtures/alacritty.toml`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: nothing
- Produces:
  ```rust
  pub struct Palette { pub bg: String, pub surface: String, pub text: String,
                       pub muted: String, pub accent: String, pub is_dark: bool }
  impl Palette { pub fn fallback_dark() -> Self }
  pub fn parse_alacritty(toml_src: &str) -> Option<Palette>
  pub fn resolve(theme_dir: Option<&Path>) -> Palette   // never fails
  ```

- [ ] **Step 1: Create the fixture**

```toml
# src-tauri/tests/fixtures/alacritty.toml
[colors.primary]
background = "#1a1b26"
foreground = "#c0caf5"

[colors.normal]
black   = "#15161e"
red     = "#f7768e"
green   = "#9ece6a"
yellow  = "#e0af68"
blue    = "#7aa2f7"
magenta = "#bb9af7"
cyan    = "#7dcfff"
white   = "#a9b1d6"
```

- [ ] **Step 2: Write the failing tests**

```rust
// src-tauri/src/theme.rs
#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/alacritty.toml");

    #[test]
    fn a_tokyo_night_alacritty_theme_parses() {
        let p = parse_alacritty(FIXTURE).unwrap();
        assert_eq!(p.bg, "#1a1b26");
        assert_eq!(p.text, "#c0caf5");
        assert_eq!(p.accent, "#7aa2f7");
        assert!(p.is_dark);
    }

    #[test]
    fn a_light_background_is_detected_as_light() {
        let src = r#"
[colors.primary]
background = "#eff1f5"
foreground = "#4c4f69"
[colors.normal]
blue = "#1e66f5"
"#;
        let p = parse_alacritty(src).unwrap();
        assert!(!p.is_dark);
    }

    #[test]
    fn a_theme_without_a_background_is_rejected() {
        assert!(parse_alacritty("[colors.normal]\nblue = \"#1e66f5\"").is_none());
    }

    #[test]
    fn malformed_toml_is_rejected_without_panicking() {
        assert!(parse_alacritty("this is not toml {{{").is_none());
    }

    #[test]
    fn a_missing_accent_falls_back_to_the_foreground() {
        let src = "[colors.primary]\nbackground = \"#1a1b26\"\nforeground = \"#c0caf5\"";
        let p = parse_alacritty(src).unwrap();
        assert_eq!(p.accent, "#c0caf5");
    }

    #[test]
    fn resolve_falls_back_when_the_directory_is_missing() {
        let p = resolve(Some(std::path::Path::new("/nonexistent/omarchy/theme")));
        assert_eq!(p, Palette::fallback_dark());
    }

    #[test]
    fn resolve_falls_back_when_given_nothing() {
        assert_eq!(resolve(None), Palette::fallback_dark());
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p omacal theme`
Expected: FAIL — `cannot find function parse_alacritty`.

- [ ] **Step 4: Implement**

```bash
cargo add --package omacal toml serde --features serde/derive
```

```rust
// src-tauri/src/theme.rs  (above the tests module)
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Palette {
    pub bg: String,
    pub surface: String,
    pub text: String,
    pub muted: String,
    pub accent: String,
    pub is_dark: bool,
}

impl Palette {
    /// Used whenever a theme cannot be read or parsed. The app must always
    /// start (spec §10).
    pub fn fallback_dark() -> Self {
        Self {
            bg: "#17171a".into(),
            surface: "#1e1e22".into(),
            text: "#e8e8ea".into(),
            muted: "#8a8a90".into(),
            accent: "#5b8def".into(),
            is_dark: true,
        }
    }
}

#[derive(Deserialize)]
struct AlacrittyFile {
    colors: Option<AlacrittyColors>,
}

#[derive(Deserialize)]
struct AlacrittyColors {
    primary: Option<AlacrittyPrimary>,
    normal: Option<AlacrittyNormal>,
}

#[derive(Deserialize)]
struct AlacrittyPrimary {
    background: Option<String>,
    foreground: Option<String>,
}

#[derive(Deserialize)]
struct AlacrittyNormal {
    blue: Option<String>,
    white: Option<String>,
}

/// Relative luminance of `#rrggbb`, used only to classify dark vs light.
fn luminance(hex: &str) -> Option<f32> {
    let h = hex.trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&h[0..2], 16).ok()? as f32;
    let g = u8::from_str_radix(&h[2..4], 16).ok()? as f32;
    let b = u8::from_str_radix(&h[4..6], 16).ok()? as f32;
    Some((0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0)
}

/// Lightens or darkens `hex` by `amount` (-1.0..=1.0), used to derive a surface
/// colour one step away from the background.
fn shift(hex: &str, amount: f32) -> String {
    let h = hex.trim_start_matches('#');
    if h.len() != 6 {
        return hex.to_string();
    }
    let ch = |i: usize| -> u8 {
        let v = u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0) as f32;
        (v + 255.0 * amount).clamp(0.0, 255.0) as u8
    };
    format!("#{:02x}{:02x}{:02x}", ch(0), ch(2), ch(4))
}

/// Parses an Alacritty theme into a palette. Returns `None` when the file is
/// not valid TOML or carries no background colour.
pub fn parse_alacritty(toml_src: &str) -> Option<Palette> {
    let file: AlacrittyFile = toml::from_str(toml_src).ok()?;
    let colors = file.colors?;
    let primary = colors.primary?;
    let bg = primary.background?;
    let text = primary.foreground.unwrap_or_else(|| "#e8e8ea".into());
    let is_dark = luminance(&bg).map(|l| l < 0.5).unwrap_or(true);
    let accent = colors
        .normal
        .as_ref()
        .and_then(|n| n.blue.clone())
        .unwrap_or_else(|| text.clone());
    let muted = colors
        .normal
        .as_ref()
        .and_then(|n| n.white.clone())
        .unwrap_or_else(|| shift(&text, if is_dark { -0.25 } else { 0.25 }));

    Some(Palette {
        surface: shift(&bg, if is_dark { 0.03 } else { -0.03 }),
        bg,
        text,
        muted,
        accent,
        is_dark,
    })
}

/// Resolves the active palette, following the spec §10 fallback chain:
/// `alacritty.toml` in the theme directory, then the built-in dark palette.
/// Never fails.
pub fn resolve(theme_dir: Option<&Path>) -> Palette {
    let Some(dir) = theme_dir else {
        return Palette::fallback_dark();
    };
    match std::fs::read_to_string(dir.join("alacritty.toml")) {
        Ok(src) => parse_alacritty(&src).unwrap_or_else(|| {
            tracing::warn!(?dir, "theme found but could not be parsed; using fallback");
            Palette::fallback_dark()
        }),
        Err(e) => {
            tracing::warn!(?dir, %e, "no readable theme; using fallback");
            Palette::fallback_dark()
        }
    }
}

/// The conventional Omarchy location. Returns `None` off Linux or when absent.
pub fn omarchy_theme_dir() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")?;
    let p = Path::new(&home).join(".config/omarchy/current/theme");
    p.exists().then_some(p)
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p omacal theme`
Expected: 7 passed.

- [ ] **Step 6: Commit**

```bash
git add src-tauri
git commit -m "feat(theme): Omarchy theme resolution with safe fallback"
```

---

### Task 11: Tauri command surface

**Files:**
- Create: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `omacal_core::{lay_out_day, pack_lanes, expand}`, `omacal_store`, `theme::resolve`
- Produces (callable from TypeScript via `invoke`):
  ```rust
  #[tauri::command] async fn get_palette() -> Palette
  #[tauri::command] async fn get_week(state: State<'_, AppState>, week_start_ms: i64)
      -> Result<WeekPayload, String>
  ```
  ```rust
  pub struct UiEvent { pub id: i64, pub title: String, pub location: Option<String>,
                       pub start_ms: i64, pub end_ms: i64, pub color: String,
                       pub response: String, pub guests: u32, pub is_all_day: bool }
  pub struct WeekPayload { pub days: [DayColumn; 7], pub all_day: Vec<Lane>,
                           pub all_day_events: Vec<UiEvent>, pub overflow: Vec<usize> }
  pub struct DayColumn { pub start_ms: i64, pub events: Vec<UiEvent>, pub placed: Vec<Placed> }
  ```

- [ ] **Step 1: Write the failing test for week assembly**

The pure part — turning stored events into seven laid-out columns — is testable without Tauri.

```rust
// src-tauri/src/commands.rs
#[cfg(test)]
mod tests {
    use super::*;

    fn ev(gid: &str, start: i64, end: i64, all_day: bool) -> omacal_store::StoredEvent {
        omacal_store::StoredEvent {
            id: 0, calendar_id: 1, google_id: gid.into(), summary: Some(gid.into()),
            location: None, start_utc: start, end_utc: end,
            start_tz: "UTC".into(), end_tz: "UTC".into(),
            is_all_day: all_day, recurrence: None, status: "confirmed".into(),
            self_response: Some("accepted".into()), conference_uri: None,
        }
    }

    const DAY: i64 = 24 * 3_600_000;
    /// Monday 2026-08-03 00:00:00 UTC
    const MON: i64 = 1_785_715_200_000;

    #[test]
    fn a_timed_event_lands_in_its_own_day_column() {
        let evs = vec![ev("a", MON + 9 * 3_600_000, MON + 10 * 3_600_000, false)];
        let w = assemble_week(&evs, MON, "UTC");
        assert_eq!(w.days[0].events.len(), 1);
        assert!(w.days[1].events.is_empty());
    }

    #[test]
    fn an_event_on_wednesday_lands_in_column_two() {
        let evs = vec![ev("a", MON + 2 * DAY + 9 * 3_600_000, MON + 2 * DAY + 10 * 3_600_000, false)];
        let w = assemble_week(&evs, MON, "UTC");
        assert_eq!(w.days[2].events.len(), 1);
    }

    #[test]
    fn overlapping_events_get_two_columns() {
        let evs = vec![
            ev("a", MON + 10 * 3_600_000, MON + 11 * 3_600_000, false),
            ev("b", MON + 10 * 3_600_000, MON + 11 * 3_600_000, false),
        ];
        let w = assemble_week(&evs, MON, "UTC");
        assert_eq!(w.days[0].placed[0].columns, 2);
    }

    #[test]
    fn all_day_events_go_to_the_band_not_the_grid() {
        let evs = vec![ev("trip", MON, MON + 3 * DAY, true)];
        let w = assemble_week(&evs, MON, "UTC");
        assert!(w.days[0].events.is_empty());
        assert_eq!(w.all_day_events.len(), 1);
        assert_eq!(w.all_day[0].start_col, 0);
        assert_eq!(w.all_day[0].end_col, 2);
    }

    #[test]
    fn an_all_day_span_entering_the_week_is_flagged_as_continuing() {
        let evs = vec![ev("trip", MON - 3 * DAY, MON + 2 * DAY, true)];
        let w = assemble_week(&evs, MON, "UTC");
        assert!(w.all_day[0].cont_left);
        assert!(!w.all_day[0].cont_right);
    }

    #[test]
    fn events_outside_the_week_are_dropped() {
        let evs = vec![ev("a", MON + 30 * DAY, MON + 30 * DAY + 3_600_000, false)];
        let w = assemble_week(&evs, MON, "UTC");
        assert!(w.days.iter().all(|d| d.events.is_empty()));
    }

    #[test]
    fn a_recurring_master_is_expanded_across_the_week() {
        let mut master = ev("standup", MON + 9 * 3_600_000, MON + 9 * 3_600_000 + 1_800_000, false);
        master.recurrence = Some("RRULE:FREQ=DAILY".into());
        let w = assemble_week(&[master], MON, "UTC");
        let total: usize = w.days.iter().map(|d| d.events.len()).sum();
        assert_eq!(total, 7);
    }

    /// Monday 2026-10-19 00:00 in Europe/Sofia. That week contains the
    /// end of DST on Sunday 2026-10-25, making that Sunday 25 hours long.
    fn dst_week_start() -> i64 {
        jiff::civil::date(2026, 10, 19)
            .at(0, 0, 0, 0)
            .in_tz("Europe/Sofia")
            .unwrap()
            .timestamp()
            .as_millisecond()
    }

    #[test]
    fn a_dst_week_contains_a_twenty_five_hour_day() {
        let bounds = day_boundaries(dst_week_start(), "Europe/Sofia");
        let lengths: Vec<i64> = bounds.windows(2).map(|w| w[1] - w[0]).collect();
        assert_eq!(lengths.len(), 7);
        assert!(
            lengths.contains(&(25 * 3_600_000)),
            "expected one 25-hour day, got {lengths:?}"
        );
    }

    #[test]
    fn a_normal_week_has_seven_equal_days() {
        let bounds = day_boundaries(dst_week_start() - 7 * DAY, "Europe/Sofia");
        let lengths: Vec<i64> = bounds.windows(2).map(|w| w[1] - w[0]).collect();
        assert!(lengths.iter().all(|&l| l == DAY), "got {lengths:?}");
    }

    #[test]
    fn an_event_late_on_a_long_day_stays_inside_its_column() {
        let start = dst_week_start();
        let bounds = day_boundaries(start, "Europe/Sofia");
        // 24h30m into the 25-hour Sunday: valid, and only representable
        // because the day window is its true length.
        let late = bounds[6] + 24 * 3_600_000 + 1_800_000;
        assert_eq!(column_for(&bounds, late), Some(6));

        let w = assemble_week(&[ev("night", late, late + 1_800_000, false)], start, "Europe/Sofia");
        assert_eq!(w.days[6].events.len(), 1);
        let p = w.days[6].placed[0];
        assert!(p.top < 1.0, "top {} should stay within the day", p.top);
        assert!(p.top + p.height <= 1.0001, "block overflows the column");
    }

    #[test]
    fn an_unknown_timezone_still_produces_seven_days() {
        let bounds = day_boundaries(MON, "Mars/Olympus_Mons");
        assert_eq!(bounds.len(), 8);
        assert_eq!(bounds[7] - bounds[0], 7 * DAY);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p omacal commands`
Expected: FAIL — `cannot find function assemble_week`.

- [ ] **Step 3: Implement**

```rust
// src-tauri/src/commands.rs  (above the tests module)
use omacal_core::{expand, lay_out_day, pack_lanes, Interval, Lane, Placed, Segment, Series};
use omacal_store::StoredEvent;
use serde::Serialize;

const DAY_MS: i64 = 24 * 3_600_000;
/// Expansion guard for one week of any single series. Sized for the realistic
/// worst case — a 30-minute block recurring through every working hour is ~336
/// occurrences a week — so that `Expansion::truncated` stays false in practice.
const EXPAND_LIMIT: u16 = 512;

#[derive(Debug, Clone, Serialize)]
pub struct UiEvent {
    pub id: i64,
    pub title: String,
    pub location: Option<String>,
    pub start_ms: i64,
    pub end_ms: i64,
    pub color: String,
    /// `accepted` | `needsAction` | `tentative` | `declined`
    pub response: String,
    pub guests: u32,
    pub is_all_day: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DayColumn {
    pub start_ms: i64,
    pub events: Vec<UiEvent>,
    pub placed: Vec<Placed>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WeekPayload {
    pub days: Vec<DayColumn>,
    pub all_day: Vec<Lane>,
    pub all_day_events: Vec<UiEvent>,
    pub overflow: Vec<usize>,
}

fn to_ui(src: &StoredEvent, start_ms: i64, end_ms: i64) -> UiEvent {
    UiEvent {
        id: src.id,
        title: src.summary.clone().unwrap_or_else(|| "(no title)".into()),
        location: src.location.clone(),
        start_ms,
        end_ms,
        color: "#5b8def".into(), // replaced by the calendar's colour in Task 13
        response: src.self_response.clone().unwrap_or_else(|| "accepted".into()),
        guests: 0,
        is_all_day: src.is_all_day,
    }
}

/// Expands one stored row into the concrete occurrences overlapping the window.
fn occurrences(src: &StoredEvent, from_ms: i64, to_ms: i64) -> Vec<Interval> {
    let Some(rule) = &src.recurrence else {
        let iv = Interval { start_ms: src.start_utc, end_ms: src.end_utc };
        return if iv.start_ms < to_ms && iv.end_ms > from_ms { vec![iv] } else { vec![] };
    };
    let lines: Vec<String> = rule.lines().map(|s| s.to_string()).collect();
    let series = Series {
        dtstart_ms: src.start_utc,
        dtstart_tz: &src.start_tz,
        duration_ms: src.end_utc - src.start_utc,
        is_all_day: src.is_all_day,
        recurrence: &lines,
    };
    match expand(&series, from_ms, to_ms, EXPAND_LIMIT) {
        Ok(e) => {
            if e.truncated {
                // Surfaced rather than swallowed: a series this dense means the
                // window is showing an incomplete picture.
                tracing::warn!(
                    google_id = %src.google_id,
                    limit = EXPAND_LIMIT,
                    "recurrence expansion truncated"
                );
            }
            e.intervals
        }
        Err(err) => {
            tracing::warn!(google_id = %src.google_id, %err, "recurrence expansion failed");
            Vec::new()
        }
    }
}

/// The eight instants bounding the week's seven days, computed **in `tz`**.
///
/// Never `week_start + n * DAY_MS`: on a DST transition day that arithmetic is
/// off by an hour, which both misplaces events and squashes or stretches the
/// day's geometry. `Zoned::checked_add(1.day())` does calendar-day arithmetic,
/// so a 23- or 25-hour day comes out at its true length.
///
/// Falls back to fixed 24-hour days only if the zone is unknown — the grid must
/// still render.
fn day_boundaries(week_start_ms: i64, tz: &str) -> Vec<i64> {
    use jiff::{Timestamp, ToSpan};

    let fallback = || (0..=7).map(|i| week_start_ms + i * DAY_MS).collect::<Vec<_>>();

    let Ok(start) = Timestamp::from_millisecond(week_start_ms) else {
        return fallback();
    };
    let Ok(mut z) = start.in_tz(tz) else {
        return fallback();
    };

    let mut out = Vec::with_capacity(8);
    out.push(z.timestamp().as_millisecond());
    for _ in 0..7 {
        match z.checked_add(1.day()) {
            Ok(next) => {
                z = next;
                out.push(z.timestamp().as_millisecond());
            }
            Err(_) => {
                let last = *out.last().unwrap();
                out.push(last + DAY_MS);
            }
        }
    }
    out
}

/// Column `0..=6` for an instant inside the week, or `None` outside it.
fn column_for(bounds: &[i64], ms: i64) -> Option<usize> {
    if ms < bounds[0] || ms >= bounds[7] {
        return None;
    }
    Some(bounds.partition_point(|&b| b <= ms) - 1)
}

/// Column index that may fall outside `0..=6`, for all-day spans that begin
/// before or end after this week. Only the sign matters to `pack_lanes`, which
/// turns it into a continuation flag, so approximating the days beyond the
/// week's edges with fixed 24-hour arithmetic is harmless.
fn signed_column(bounds: &[i64], ms: i64) -> i32 {
    if ms < bounds[0] {
        -(((bounds[0] - ms - 1) / DAY_MS) + 1) as i32
    } else if ms >= bounds[7] {
        7 + ((ms - bounds[7]) / DAY_MS) as i32
    } else {
        (bounds.partition_point(|&b| b <= ms) - 1) as i32
    }
}

/// Turns stored events into seven laid-out day columns plus the all-day band.
///
/// `week_start_ms` is midnight in `tz` on the week's Monday; `tz` is the display
/// zone. All day-boundary maths flows through `day_boundaries`, so a week
/// containing a DST transition lays out correctly.
pub fn assemble_week(events: &[StoredEvent], week_start_ms: i64, tz: &str) -> WeekPayload {
    let bounds = day_boundaries(week_start_ms, tz);
    let week_end_ms = bounds[7];

    let mut day_events: Vec<Vec<UiEvent>> = vec![Vec::new(); 7];
    let mut all_day_events: Vec<UiEvent> = Vec::new();
    let mut segments: Vec<Segment> = Vec::new();

    for src in events {
        for iv in occurrences(src, bounds[0], week_end_ms) {
            if src.is_all_day {
                let start_col = signed_column(&bounds, iv.start_ms);
                // Google's all-day end is exclusive, so the last covered day is
                // one millisecond before it.
                let end_col = signed_column(&bounds, iv.end_ms - 1);
                segments.push(Segment { idx: all_day_events.len(), start_col, end_col });
                all_day_events.push(to_ui(src, iv.start_ms, iv.end_ms));
            } else if let Some(col) = column_for(&bounds, iv.start_ms) {
                day_events[col].push(to_ui(src, iv.start_ms, iv.end_ms));
            }
        }
    }

    let (all_day, overflow) = pack_lanes(&segments, 7, 2);

    let days = (0..7)
        .map(|d| {
            let evs = std::mem::take(&mut day_events[d]);
            let intervals: Vec<Interval> = evs
                .iter()
                .map(|e| Interval { start_ms: e.start_ms, end_ms: e.end_ms })
                .collect();
            // The window is the day's *true* length, so a 25-hour day is not
            // compressed into 24 hours' worth of geometry.
            let placed = lay_out_day(&intervals, bounds[d], bounds[d + 1]);
            DayColumn { start_ms: bounds[d], events: evs, placed }
        })
        .collect();

    WeekPayload { days, all_day, all_day_events, overflow }
}
```

`src-tauri` needs `jiff` for this:

```bash
cargo add --package omacal jiff
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p omacal commands`
Expected: 7 passed.

- [ ] **Step 5: Expose the Tauri commands**

```rust
// src-tauri/src/lib.rs — add
mod commands;
mod theme;

use sqlx::SqlitePool;

pub struct AppState {
    pub pool: SqlitePool,
}

#[tauri::command]
fn get_palette() -> theme::Palette {
    theme::resolve(theme::omarchy_theme_dir().as_deref())
}

/// The display zone: the user's setting if present, otherwise the system zone.
/// Every day boundary in the week grid is computed against this.
fn display_tz(pool: &SqlitePool) -> String {
    // `settings` is read on the sync task's runtime elsewhere; here we only
    // need a cheap default, so fall back to the system zone.
    let _ = pool;
    jiff::tz::TimeZone::system()
        .iana_name()
        .unwrap_or("UTC")
        .to_string()
}

#[tauri::command]
async fn get_week(
    state: tauri::State<'_, AppState>,
    week_start_ms: i64,
) -> Result<commands::WeekPayload, String> {
    let tz = display_tz(&state.pool);
    // Widen the fetch by a day either side so an event that begins just before
    // the week (or a DST-lengthened final day) is not missed.
    const DAY: i64 = 24 * 3_600_000;
    let events = omacal_store::events_in_window(
        &state.pool,
        week_start_ms - DAY,
        week_start_ms + 8 * DAY,
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(commands::assemble_week(&events, week_start_ms, &tz))
}
```

- [ ] **Step 6: Add sign-in, credential storage, and account bootstrap**

```bash
cargo add --package omacal keyring open toml
```

```rust
// src-tauri/src/lib.rs — continued

const KEYRING_SERVICE: &str = "omacal";

#[derive(serde::Deserialize)]
struct Config {
    client_id: String,
    client_secret: String,
}

/// Reads `~/.config/omacal/config.toml`, which holds the Google Cloud client
/// credentials (spec §9 — single-user, credentials supplied by config file).
fn load_config() -> anyhow::Result<Config> {
    let home = std::env::var("HOME")?;
    let path = std::path::Path::new(&home).join(".config/omacal/config.toml");
    let src = std::fs::read_to_string(&path).map_err(|e| {
        anyhow::anyhow!("no config at {}: {e}. Create it with client_id and client_secret.", path.display())
    })?;
    Ok(toml::from_str(&src)?)
}

fn store_refresh_token(email: &str, token: &str) -> anyhow::Result<()> {
    keyring::Entry::new(KEYRING_SERVICE, email)?.set_password(token)?;
    Ok(())
}

fn load_refresh_token(email: &str) -> anyhow::Result<String> {
    Ok(keyring::Entry::new(KEYRING_SERVICE, email)?.get_password()?)
}

/// Runs the full interactive sign-in: loopback listener, browser, code
/// exchange, keyring write, then account and calendar bootstrap.
#[tauri::command]
async fn sign_in(state: tauri::State<'_, AppState>) -> Result<String, String> {
    async fn inner(pool: &SqlitePool) -> anyhow::Result<String> {
        let cfg = load_config()?;
        let pkce = omacal_google::auth::generate_pkce();
        let (listener, redirect_uri) = omacal_google::auth::bind_loopback()?;
        let csrf = omacal_google::auth::generate_pkce().verifier;

        let url = omacal_google::auth::authorize_url(
            &cfg.client_id, &redirect_uri, &pkce.challenge, &csrf,
        );
        open::that(&url)?;

        let redirect = tokio::task::spawn_blocking(move || {
            omacal_google::auth::wait_for_redirect(listener)
        })
        .await??;

        if redirect.state != csrf {
            anyhow::bail!("state mismatch — possible CSRF, sign-in aborted");
        }

        let tokens = omacal_google::auth::exchange_code(
            omacal_google::auth::TOKEN_ENDPOINT,
            &cfg.client_id, &cfg.client_secret,
            &redirect.code, &pkce.verifier, &redirect_uri,
        )
        .await?;

        let client = omacal_google::CalendarClient::new(
            "https://www.googleapis.com/calendar/v3",
            &tokens.access_token,
        );
        let calendars = client.list_calendars().await?;

        // The primary calendar's id is the account's email address, so we get
        // the identity without requesting a userinfo scope.
        let email = calendars
            .iter()
            .find(|c| c.primary)
            .map(|c| c.id.clone())
            .ok_or_else(|| anyhow::anyhow!("account has no primary calendar"))?;

        if let Some(rt) = &tokens.refresh_token {
            store_refresh_token(&email, rt)?;
        } else {
            anyhow::bail!("Google returned no refresh token — revoke the app's access and retry");
        }

        // `google_sub` keys the account. We use the email, which is stable for
        // our single-user case; Plan 5 may switch to the real `sub` from an
        // id_token when multiple accounts land.
        let account_id: i64 = sqlx::query_scalar(
            "INSERT INTO accounts (google_sub, email, created_at) VALUES (?1, ?1, ?2)
             ON CONFLICT (google_sub) DO UPDATE SET email = excluded.email
             RETURNING id",
        )
        .bind(&email)
        .bind(now_ms())
        .fetch_one(pool)
        .await?;

        for c in &calendars {
            sqlx::query(
                "INSERT INTO calendars
                     (account_id, google_id, summary, color_hex, timezone, access_role, is_primary)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)
                 ON CONFLICT (account_id, google_id) DO UPDATE SET
                     summary = excluded.summary, color_hex = excluded.color_hex,
                     timezone = excluded.timezone, access_role = excluded.access_role",
            )
            .bind(account_id)
            .bind(&c.id)
            .bind(&c.summary)
            .bind(&c.background_color)
            .bind(c.time_zone.as_deref().unwrap_or("UTC"))
            .bind(&c.access_role)
            .bind(c.primary as i64)
            .execute(pool)
            .await?;
        }

        Ok(email)
    }

    inner(&state.pool).await.map_err(|e| e.to_string())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Refreshes the access token and syncs every calendar of every account.
#[tauri::command]
async fn sync_now(state: tauri::State<'_, AppState>) -> Result<u64, String> {
    async fn inner(pool: &SqlitePool) -> anyhow::Result<u64> {
        let cfg = load_config()?;
        let accounts: Vec<(i64, String)> =
            sqlx::query_as("SELECT id, email FROM accounts").fetch_all(pool).await?;

        const DAY: i64 = 24 * 3_600_000;
        let now = now_ms();
        let (window_start, window_end) = (now - 180 * DAY, now + 365 * DAY);
        let mut total = 0u64;

        for (account_id, email) in accounts {
            let refresh_token = load_refresh_token(&email)?;
            let tokens = omacal_google::auth::refresh(
                omacal_google::auth::TOKEN_ENDPOINT,
                &cfg.client_id, &cfg.client_secret, &refresh_token,
            )
            .await?;
            let client = omacal_google::CalendarClient::new(
                "https://www.googleapis.com/calendar/v3",
                &tokens.access_token,
            );

            let cals: Vec<(i64, String)> = sqlx::query_as(
                "SELECT id, google_id FROM calendars WHERE account_id = ?1 AND selected = 1",
            )
            .bind(account_id)
            .fetch_all(pool)
            .await?;

            for (cal_id, google_id) in cals {
                let out = omacal_sync::sync_calendar(
                    pool, &client, cal_id, &google_id, window_start, window_end,
                )
                .await?;
                total += (out.upserted + out.deleted) as u64;
            }
        }
        Ok(total)
    }

    inner(&state.pool).await.map_err(|e| e.to_string())
}
```

- [ ] **Step 7: Wire the builder**

```rust
// src-tauri/src/lib.rs — the entry point

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt::init();

    tauri::Builder::default()
        .setup(|app| {
            use tauri::Manager;
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let url = format!("sqlite://{}", dir.join("omacal.db").display());

            // Block once at startup: nothing can render before migrations run.
            let pool = tauri::async_runtime::block_on(omacal_store::connect(&url))?;
            app.manage(AppState { pool });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_palette,
            get_week,
            sign_in,
            sync_now
        ])
        .run(tauri::generate_context!())
        .expect("error while running omacal");
}
```

```bash
cargo add --package omacal tracing-subscriber
cargo add --package omacal --path crates/omacal-sync
cargo add --package omacal --path crates/omacal-google
cargo add --package omacal --path crates/omacal-store
cargo add --package omacal --path crates/omacal-core
```

- [ ] **Step 8: Verify sign-in end to end**

Create `~/.config/omacal/config.toml`:

```toml
client_id = "<your-client-id>.apps.googleusercontent.com"
client_secret = "<your-client-secret>"
```

The Google Cloud project must be **published to Production**, not left in Testing — otherwise the refresh token expires after 7 days (spec §9).

Run: `cargo tauri dev`, then from the app call `sign_in` followed by `sync_now`.
Expected: the browser opens Google's consent screen; after granting, the tab says "Signed in"; `sync_now` returns a non-zero count; `accounts`, `calendars` and `events` are populated.

Verify persistence:

```bash
sqlite3 "$HOME/Library/Application Support/com.omacal.app/omacal.db" \
  "SELECT COUNT(*) FROM events;"
```

- [ ] **Step 9: Commit**

```bash
cargo test --workspace
git add src-tauri
git commit -m "feat(app): week assembly, sign-in, and sync commands"
```

---

### Task 12: Week grid shell

**Files:**
- Create: `ui/src/lib/theme.ts`, `ui/src/lib/api.ts`, `ui/src/lib/WeekGrid.svelte`
- Modify: `ui/src/App.svelte`, `ui/src/app.css`

**Interfaces:**
- Consumes: `get_palette`, `get_week` Tauri commands
- Produces: `WeekGrid` component rendering the quiet grid of spec §7.1

- [ ] **Step 1: Bind the palette to CSS variables**

```ts
// ui/src/lib/theme.ts
import { invoke } from '@tauri-apps/api/core';

export type Palette = {
  bg: string; surface: string; text: string;
  muted: string; accent: string; is_dark: boolean;
};

/** Pushes the resolved palette onto :root so all styling flows from CSS vars. */
export async function applyPalette(): Promise<Palette> {
  const p = await invoke<Palette>('get_palette');
  const r = document.documentElement.style;
  r.setProperty('--bg', p.bg);
  r.setProperty('--surface', p.surface);
  r.setProperty('--text', p.text);
  r.setProperty('--muted', p.muted);
  r.setProperty('--accent', p.accent);
  r.setProperty('--hairline', p.is_dark ? 'rgba(255,255,255,.055)' : 'rgba(0,0,0,.07)');
  r.setProperty('--hour-rule', p.is_dark ? 'rgba(255,255,255,.035)' : 'rgba(0,0,0,.05)');
  r.setProperty('--today-tint', p.is_dark ? 'rgba(255,255,255,.028)' : 'rgba(0,0,0,.025)');
  return p;
}
```

- [ ] **Step 2: Type the week payload**

```ts
// ui/src/lib/api.ts
import { invoke } from '@tauri-apps/api/core';

export type UiEvent = {
  id: number; title: string; location: string | null;
  start_ms: number; end_ms: number; color: string;
  response: 'accepted' | 'needsAction' | 'tentative' | 'declined';
  guests: number; is_all_day: boolean;
};
export type Placed = { idx: number; column: number; columns: number; top: number; height: number };
export type Lane = {
  idx: number; lane: number; start_col: number; end_col: number;
  cont_left: boolean; cont_right: boolean;
};
export type DayColumn = { start_ms: number; events: UiEvent[]; placed: Placed[] };
export type WeekPayload = {
  days: DayColumn[]; all_day: Lane[]; all_day_events: UiEvent[]; overflow: number[];
};

/** Midnight local on the Monday of the week containing `d`. */
export function weekStart(d: Date): number {
  const m = new Date(d);
  m.setHours(0, 0, 0, 0);
  m.setDate(m.getDate() - ((m.getDay() + 6) % 7));
  return m.getTime();
}

export const getWeek = (weekStartMs: number) =>
  invoke<WeekPayload>('get_week', { weekStartMs });
```

- [ ] **Step 3: Build the grid**

```svelte
<!-- ui/src/lib/WeekGrid.svelte -->
<script lang="ts">
  import type { WeekPayload } from './api';
  import EventBlock from './EventBlock.svelte';
  import AllDayBand from './AllDayBand.svelte';

  let { week, weekStartMs }: { week: WeekPayload; weekStartMs: number } = $props();

  const DAY = 86_400_000;
  const HOURS = [0, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 22];
  const NAMES = ['MON', 'TUE', 'WED', 'THU', 'FRI', 'SAT', 'SUN'];

  const todayStart = (() => { const d = new Date(); d.setHours(0,0,0,0); return d.getTime(); })();

  // Current-time line as a fraction of the day, recomputed each minute.
  let nowFrac = $state(0);
  $effect(() => {
    const tick = () => {
      const n = new Date();
      nowFrac = (n.getHours() * 60 + n.getMinutes()) / 1440;
    };
    tick();
    const id = setInterval(tick, 60_000);
    return () => clearInterval(id);
  });
</script>

<div class="grid">
  <div class="gutter head"></div>
  {#each NAMES as name, i}
    {@const dayStart = weekStartMs + i * DAY}
    <div class="head" class:today={dayStart === todayStart}>
      <span>{name}</span>
      <b>{new Date(dayStart).getDate()}</b>
    </div>
  {/each}
</div>

<AllDayBand lanes={week.all_day} events={week.all_day_events} overflow={week.overflow} />

<div class="grid body">
  <div class="gutter">
    {#each HOURS as h}
      <span style="top:{(h / 24) * 100}%">{String(h).padStart(2, '0')}</span>
    {/each}
  </div>

  {#each week.days as day, i}
    {@const isToday = day.start_ms === todayStart}
    <div class="col" class:today={isToday}>
      {#each HOURS as h}
        <div class="rule" style="top:{(h / 24) * 100}%"></div>
      {/each}

      {#each day.placed as p}
        <EventBlock event={day.events[p.idx]} placed={p} />
      {/each}

      {#if isToday}
        <div class="now" style="top:{nowFrac * 100}%"></div>
      {/if}
    </div>
  {/each}
</div>

<style>
  .grid { display: grid; grid-template-columns: 44px repeat(7, 1fr); }
  .body { height: calc(100vh - 150px); overflow-y: auto; position: relative; }

  .head { text-align: center; font-size: 10px; color: var(--muted);
          letter-spacing: .05em; padding-bottom: 8px; }
  .head b { display: block; font-size: 15px; color: var(--text);
            font-weight: 500; letter-spacing: -.02em; margin-top: 2px; }
  .head.today b { background: var(--accent); color: var(--bg); width: 23px; height: 23px;
                  line-height: 23px; border-radius: 50%; margin: 2px auto 0; font-weight: 600; }

  /* No column borders: the grid reads through alignment, not rules (spec §7.1). */
  .col { position: relative; min-height: 1200px; }
  .col.today { background: var(--today-tint); border-radius: 6px; }

  .gutter { position: relative; }
  .gutter span { position: absolute; right: 8px; font-size: 9.5px; color: var(--muted);
                 opacity: .7; transform: translateY(-50%); font-variant-numeric: tabular-nums; }

  .rule { position: absolute; left: 0; right: 0; border-top: 1px solid var(--hour-rule); }

  /* The loudest thing on screen, deliberately. */
  .now { position: absolute; left: 0; right: 0; border-top: 1.5px solid #e2564a; z-index: 5; }
  .now::before { content: ''; position: absolute; left: -3px; top: -3.5px;
                 width: 7px; height: 7px; border-radius: 50%; background: #e2564a; }
</style>
```

- [ ] **Step 4: Wire into the app**

```svelte
<!-- ui/src/App.svelte -->
<script lang="ts">
  import { applyPalette } from './lib/theme';
  import { getWeek, weekStart, type WeekPayload } from './lib/api';
  import WeekGrid from './lib/WeekGrid.svelte';

  let weekStartMs = $state(weekStart(new Date()));
  let week = $state<WeekPayload | null>(null);
  let error = $state<string | null>(null);

  $effect(() => { applyPalette(); });

  $effect(() => {
    getWeek(weekStartMs)
      .then((w) => { week = w; error = null; })
      .catch((e) => { error = String(e); });
  });
</script>

<main>
  {#if error}
    <p class="error">{error}</p>
  {:else if week}
    <WeekGrid {week} {weekStartMs} />
  {/if}
</main>

<style>
  :global(body) { background: var(--bg); color: var(--text); margin: 0;
                  font-family: -apple-system, 'SF Pro Text', Inter, system-ui, sans-serif; }
  main { padding: 14px 16px; }
  .error { color: #e2564a; font-size: 13px; }
</style>
```

- [ ] **Step 5: Verify visually**

Run: `cargo tauri dev`
Expected: a themed empty week grid with day headers, two-hourly rules, a tinted today column, and a red now-line at the correct height.

- [ ] **Step 6: Commit**

```bash
git add ui src-tauri
git commit -m "feat(ui): quiet week grid with theme-driven CSS variables"
```

---

### Task 13: Event blocks

Implements the density ladder and RSVP states of spec §7.1–§7.2.

**Files:**
- Create: `ui/src/lib/EventBlock.svelte`

**Interfaces:**
- Consumes: `UiEvent`, `Placed` from `api.ts`
- Produces: `EventBlock` component

- [ ] **Step 1: Implement**

```svelte
<!-- ui/src/lib/EventBlock.svelte -->
<script lang="ts">
  import type { UiEvent, Placed } from './api';

  let { event, placed }: { event: UiEvent; placed: Placed } = $props();

  const minutes = $derived((event.end_ms - event.start_ms) / 60_000);

  // Density ladder (spec §7.1). Thresholds are in minutes.
  const showMeta = $derived(minutes >= 45);
  const showTime = $derived(minutes >= 90);

  const hhmm = (ms: number) =>
    new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: false });

  // Location first: it is the thing you act on when you are walking somewhere.
  const meta = $derived(
    [event.location, event.guests > 0 ? `${event.guests}` : null].filter(Boolean).join(' · ')
  );

  const width = $derived(100 / placed.columns);
  const left = $derived(placed.column * width);
</script>

<button
  class="ev {event.response}"
  style="
    top:{placed.top * 100}%; height:{placed.height * 100}%;
    left:calc({left}% + 3px); width:calc({width}% - 6px);
    --cal:{event.color}; z-index:{placed.column + 1};
  "
  title={event.title}
>
  {#if event.response === 'needsAction'}<i class="rs">?</i>{/if}
  <b>{event.title}</b>
  {#if showTime}<em>{hhmm(event.start_ms)} – {hhmm(event.end_ms)}</em>{/if}
  {#if showMeta && meta}<em>{meta}</em>{/if}
</button>

<style>
  .ev {
    position: absolute; border: 0; text-align: left; cursor: pointer;
    border-radius: 6px; padding: 2px 6px; overflow: hidden;
    border-left: 2px solid var(--cal);
    background: color-mix(in srgb, var(--cal) 7%, transparent);
    color: color-mix(in srgb, var(--cal) 65%, var(--text));
    font: inherit;
  }
  /* Hover lifts the block to full width so a squeezed 3-way pile stays
     readable without changing the layout rules (spec §7.1). */
  .ev:hover { left: 3px !important; width: calc(100% - 6px) !important; z-index: 20;
              box-shadow: 0 2px 10px rgba(0, 0, 0, .35); }

  .ev b { display: block; font-size: 10px; font-weight: 600; line-height: 1.3;
          letter-spacing: -.01em; white-space: nowrap; overflow: hidden;
          text-overflow: ellipsis; }
  .ev em { font-style: normal; display: block; font-size: 9px; opacity: .62;
           line-height: 1.35; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }

  .rs { position: absolute; top: 1px; right: 4px; font-size: 9px;
        font-style: normal; font-weight: 700; opacity: .8; }

  /* State is carried by the fill, so it survives at 15 minutes tall. */
  .ev.needsAction { background: transparent; border: 1px dashed currentColor;
                    border-left: 2px solid var(--cal); }
  .ev.tentative { background-image: repeating-linear-gradient(135deg,
                  rgba(128,128,128,.16) 0 3px, transparent 3px 7px); }
  .ev.declined { background: transparent; opacity: .4; }
  .ev.declined b { text-decoration: line-through; }
</style>
```

- [ ] **Step 2: Verify against a real account**

Run: `cargo tauri dev`
Expected: real events render; a 15-minute event shows title only; a 2-hour event shows title, time and meta; an unanswered invite is dashed with a `?`; hovering a squeezed block expands it to full width.

- [ ] **Step 3: Commit**

```bash
git add ui
git commit -m "feat(ui): event blocks with duration density and RSVP states"
```

---

### Task 14: All-day band

**Files:**
- Create: `ui/src/lib/AllDayBand.svelte`

**Interfaces:**
- Consumes: `Lane`, `UiEvent`
- Produces: `AllDayBand` component

- [ ] **Step 1: Implement**

```svelte
<!-- ui/src/lib/AllDayBand.svelte -->
<script lang="ts">
  import type { Lane, UiEvent } from './api';

  let { lanes, events, overflow }:
    { lanes: Lane[]; events: UiEvent[]; overflow: number[] } = $props();

  const laneCount = $derived(lanes.length ? Math.max(...lanes.map((l) => l.lane)) + 1 : 0);
</script>

{#if lanes.length || overflow.length}
  <div class="band" style="--lanes:{laneCount}">
    <div class="label">ALL-DAY</div>
    <div class="rows">
      {#each lanes as lane}
        {@const ev = events[lane.idx]}
        <div
          class="chip"
          class:cl={lane.cont_left}
          class:cr={lane.cont_right}
          style="
            grid-row:{lane.lane + 1};
            grid-column:{lane.start_col + 1} / {lane.end_col + 2};
            --cal:{ev.color};
          "
          title={ev.title}
        >
          {lane.cont_left ? '‹ ' : ''}{ev.title}
        </div>
      {/each}
      {#if overflow.length}
        <div class="more" style="grid-row:{laneCount + 1}; grid-column:1 / -1">
          +{overflow.length} more
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .band { display: grid; grid-template-columns: 44px 1fr;
          border-bottom: 1px solid var(--hairline); padding: 3px 0 6px; margin-bottom: 2px; }
  .label { font-size: 8.5px; color: var(--muted); opacity: .8; text-align: right;
           padding-right: 7px; letter-spacing: .05em; align-self: center; }
  .rows { display: grid; grid-template-columns: repeat(7, 1fr); gap: 2px; }

  .chip { font-size: 9.5px; border-radius: 4px; padding: 2px 7px; white-space: nowrap;
          overflow: hidden; text-overflow: ellipsis;
          border-left: 2px solid var(--cal);
          background: color-mix(in srgb, var(--cal) 16%, transparent);
          color: color-mix(in srgb, var(--cal) 60%, var(--text)); }
  /* Flat edges mark a span continuing beyond this week. */
  .chip.cl { border-top-left-radius: 0; border-bottom-left-radius: 0; border-left-style: dashed; }
  .chip.cr { border-top-right-radius: 0; border-bottom-right-radius: 0; }

  .more { font-size: 9px; color: var(--muted); opacity: .7; padding: 2px 4px; }
</style>
```

- [ ] **Step 2: Verify**

Run: `cargo tauri dev`
Expected: multi-day events span the correct columns; a span that began last week shows a flat, dashed left edge.

- [ ] **Step 3: Commit**

```bash
git add ui
git commit -m "feat(ui): all-day band with multi-day spans and overflow"
```

---

### Task 15: Omarchy verification checkpoint

The M0 spike, run on the real machine. **Run this on the Omarchy box, not the Mac.** Everything before it was platform-agnostic; this is where the Linux assumptions get tested.

**Files:**
- Modify: `src-tauri/src/theme.rs` (only if the fallback chain proves wrong)
- Create: `docs/omarchy-notes.md`

**Interfaces:**
- Consumes: everything built so far
- Produces: a verified Linux build plus recorded findings

- [ ] **Step 1: Build and run on Omarchy**

```bash
sudo pacman -S --needed webkit2gtk-4.1 base-devel curl wget file openssl \
  appmenu-gtk-module libappindicator-gtk3 librsvg
cargo tauri dev
```

Expected: the window opens and the week grid renders. If it is blank or garbled, retry with `WEBKIT_DISABLE_DMABUF_RENDERER=1 cargo tauri dev` — that is risk 3 in spec §12 confirming itself.

- [ ] **Step 2: Inspect the real theme directory**

```bash
ls -la ~/.config/omarchy/current/theme/
head -40 ~/.config/omarchy/current/theme/alacritty.toml
```

Record what is actually there in `docs/omarchy-notes.md`.

- [ ] **Step 3: Verify theme resolution against every installed theme**

```bash
for t in ~/.config/omarchy/themes/*/; do
  echo "=== $t"
  cargo run --quiet --bin theme-probe -- "$t" || echo "FAILED"
done
```

Add the probe binary:

```rust
// src-tauri/src/bin/theme-probe.rs
fn main() {
    let dir = std::env::args().nth(1).expect("usage: theme-probe <theme-dir>");
    let p = omacal::theme::resolve(Some(std::path::Path::new(&dir)));
    println!("{}", serde_json::to_string_pretty(&p).unwrap());
}
```

Expected: every installed theme yields a sensible palette, and `is_dark` matches how the theme actually looks. If a theme fails, adjust the fallback chain in `theme.rs` (spec §10 permits a canonical palette file taking priority) and re-run.

- [ ] **Step 4: Verify live theme switching**

```bash
omarchy-theme-set catppuccin && cargo run
```

Expected: the palette that `get_palette` returns matches the newly-set theme. (The file *watcher* lands in Plan 2; this only confirms resolution is correct on next start.)

- [ ] **Step 5: Verify a notification reaches mako**

```bash
notify-send -a omacal "Standup" "in 5 minutes"
```

Expected: it appears in mako. This confirms the D-Bus path that Plan 3 will use. If nothing appears, check `systemctl --user status mako`.

- [ ] **Step 6: Record findings and commit**

Write `docs/omarchy-notes.md` covering: the actual theme directory contents, whether the DMABUF workaround was needed, the mako result, and any package that had to be installed beyond the list in Step 1.

```bash
git add docs/omarchy-notes.md src-tauri
git commit -m "docs: record Omarchy platform verification findings"
```

---

## Definition of Done

- [ ] `cargo test --workspace` passes — 60+ tests across four crates
- [ ] `cargo tauri dev` opens a themed window on macOS **and** on Omarchy
- [ ] Signing in with Google syncs real events into SQLite
- [ ] The current week renders with correct overlaps, all-day spans, RSVP states and the now-line
- [ ] Killing the network and restarting still renders the last synced week
- [ ] `docs/omarchy-notes.md` records the platform findings
