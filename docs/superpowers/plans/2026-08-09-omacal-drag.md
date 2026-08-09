# Drag — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** move, resize and create events by dragging in Week and Day, without a gesture ever emailing anybody by itself.

**Spec:** [`2026-08-09-omacal-drag-design.md`](../specs/2026-08-09-omacal-drag-design.md).

**Base:** `main` @ `8385ccb` — 486 Rust tests, 636 UI tests.

## Global Constraints

- **Never** modify `~/Library/Application Support/com.omacal.app/omacal.db` or `~/.config/omacal/config.toml` (three files each — `-wal` and `-shm` too).
- `sqlx::query`/`query_as`/`query_scalar` only — **never** the `query!` macros.
- **Never** `{:?}`-log or interpolate a `Tokens` value or any token string.
- **No live network calls in tests.** wiremock for Rust, harness stubs for UI.
- Svelte 5 runes only. **Never** `{@html}`. No new hardcoded hex — `theme.ts` and `ink.ts` are where colour lives.
- Every test shown to fail against broken code. **Prove a test by deleting the rule it covers, not by perturbing it** — a perturbed rule can leave behaviour observably identical, which is how the all-day anchor in `remind.rs` looked proved while being untested. Assert the mutation on disk (`grep -F`, its own statement, with a count or line match) before the suite runs. Revert with a targeted `Edit` — never `git checkout -- <file>`, never `perl`.
- **Verify gates by exit code.** `cargo test --workspace` (486), `npm --prefix ui run test:ui` (636), `npm --prefix ui run check`, `cargo clippy --workspace --all-targets -- -D warnings`.

## Why the tasks are in this order

Task 4 makes drag write to Google. Task 5 introduces the only path that can set
`sendUpdates=all`. **Until Task 5 lands, a drag is structurally incapable of
emailing anyone** — not because a dialog stops it, but because the value is not
reachable. Do not reorder these two.

---

### Task 1: `sendUpdates` becomes a parameter

**Files:** `crates/omacal-google/src/client.rs`, `src-tauri/src/events.rs`

`patch_event` hardcodes `("sendUpdates", "all")` at `client.rs:194`. Its comment
is correct for the form and wrong for a gesture; keep the reasoning, move the
choice to the caller. `create_event` already takes one — match its shape.

- [ ] **Step 1:** a wiremock test that `patch_event` sends what it was given — one case `all`, one `none`. `query_param` alone is not enough: pair it with `.expect(1)` so a request that omitted the parameter fails, the way `client.rs:489`'s comment already explains.
- [ ] **Step 2:** run, confirm it fails.
- [ ] **Step 3:** implement. **Every existing caller keeps `all`** — the form's behaviour does not change in this task.
- [ ] **Step 4:** run, confirm it passes, and that the existing `sendUpdates=all` tests still pass unchanged.
- [ ] **Step 5:** delete the parameter from the request entirely; assert on disk; confirm both cases fail; revert.
- [ ] **Step 6:** commit.

### Task 2: the geometry, as a pure function

**Files:** new module in `ui/src/lib/`

Given a pointer offset within a grid box, the box's dimensions, the day's span
and a snap interval, produce a span. No DOM, no events, no component.

```ts
export function snapMs(ms: number, intervalMs: number): number
export function spanForMove(origin: Span, dyFrac: number, dayCols: number, dxCols: number, snapMs: number): Span
export function spanForResize(origin: Span, edge: 'start' | 'end', dyFrac: number, snapMs: number): Span
```

- [ ] **Step 1:** a table-driven spec. At minimum: snapping down and up at the boundary; a move that changes day only; a move that changes time only; a resize of each edge; **a resize that would invert the event clamping to a minimum instead**; and a move whose result must not change duration.
- [ ] **Step 2–4:** run failing, implement, run passing.
- [ ] **Step 5:** delete the snap (return the raw ms) and confirm the boundary cases fail. Delete the inversion clamp and confirm its own case fails. Separately.
- [ ] **Step 6:** commit.

### Task 3: the gesture, writing nothing

**Files:** `ui/src/lib/WeekGrid.svelte`, `ui/tests/`

Pointer handling only. A block follows the pointer and returns to where it
started. **No `invoke`, no write, no dialog** — this task cannot save anything.

- [ ] **Step 1:** specs for: a drag begins only after 4px of travel; below the threshold a click still opens the popover; Escape cancels and the block returns; a drop where it started leaves the block unmoved.
- [ ] **Step 2–4:** run failing, implement, run passing.
- [ ] **Step 5:** delete the threshold and confirm the click-opens-popover spec fails. Delete the Escape handler and confirm its own spec fails.
- [ ] **Step 6:** commit.

### Task 4: move and resize write, and cannot notify

**Files:** `ui/src/lib/WeekGrid.svelte`, `ui/src/App.svelte`, `src-tauri/src/events.rs`

A completed drag saves. **`sendUpdates` is `none`, always, with no way to reach
`all` from this path.** Not a default — the only value.

- [ ] **Step 1:** specs asserting the write happens with the moved span, **and that the value sent is `none`**. Plus: a drop where it started issues **no request at all** — witnessed by the absence of a call, not by a no-op response.
- [ ] **Step 2–4:** run failing, implement, run passing.
- [ ] **Step 5:** change the value to `all` and confirm the notify spec fails. Delete the no-op-drop guard and confirm a request appears.
- [ ] **Step 6:** commit.

### Task 5: the dialogs, and the only path to `all`

**Files:** `ui/src/lib/`, `ui/src/App.svelte`

An event **with attendees** gets a prompt on drop: *Move without notifying*
(primary), *Move and notify guests*, *Cancel*. A **recurring** occurrence gets
the three-scope prompt. When both apply it is **one dialog**, the scope prompt
carrying the notify choice.

- [ ] **Step 1:** specs for each: no attendees and not recurring → no dialog, writes with `none`; attendees → dialog, and *Move without notifying* writes `none` while *Move and notify guests* writes `all`; **Cancel issues no request at all**; recurring + attendees → exactly one dialog.
- [ ] **Step 2–4:** run failing, implement, run passing.
- [ ] **Step 5:** make Cancel fall through to the write and confirm its spec fails — that is the one that matters, and it must be witnessed by an absent call rather than a visible dialog. Then delete the attendee check so every drop prompts, and confirm the no-dialog case fails.
- [ ] **Step 6:** commit.

### Task 6: create by dragging

**Files:** `ui/src/lib/WeekGrid.svelte`, `ui/src/App.svelte`

Sweeping empty grid opens the event form pre-filled with the swept span, rather
than creating silently — a new event needs a title, and the form is where that
lives.

- [ ] **Step 1:** specs: a sweep opens the form with the swept start and end; a sweep shorter than the snap interval still yields a usable minimum span; a plain click on empty grid still creates at the default hour as it does today.
- [ ] **Step 2–4:** run failing, implement, run passing.
- [ ] **Step 5:** delete the minimum-span clamp and confirm its case fails.
- [ ] **Step 6:** commit.

---

## Definition of Done

- [ ] `cargo test --workspace` ≥ 490, 0 failed — **by exit code**
- [ ] `npm --prefix ui run test:ui` ≥ 660 — **by exit code**
- [ ] `check` 0 errors; `clippy` clean
- [ ] A drag on an event with guests cannot write without an explicit choice, witnessed by an absent request on Cancel
- [ ] The only path to `sendUpdates=all` from a drag is *Move and notify guests*
- [ ] A drop where it started issues no request
- [ ] A click still opens the popover
- [ ] The form's own save behaviour is unchanged — still `all`

> **On the bars:** estimates. If the honest number is lower, report the real one — do not pad the suite and do not lower the bar. Both have been tried on this project and both were caught.
