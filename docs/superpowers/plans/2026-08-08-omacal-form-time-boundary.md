# The Form's Time Boundary — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop the event form moving events nobody moved — three shipped defects, one lossy conversion.

**Architecture:** An all-day event carries a **date** across the boundary instead of an instant, so no zone conversion happens on that path at all. A timed event keeps its instant, and an *untouched* time passes its original instant through rather than being re-derived — which is where the drift comes from.

**Tech Stack:** Tauri v2, Rust, sqlx + SQLite, Svelte 5 runes, wiremock, Playwright.

**Spec:** `docs/superpowers/specs/2026-08-08-omacal-form-time-boundary-design.md`

**Base:** `main` @ `0cc53b9` — 376 Rust tests, 454 UI tests.

## Global Constraints

- **Never** modify `~/Library/Application Support/com.omacal.app/omacal.db` or `~/.config/omacal/config.toml`. To inspect the DB, copy it **and its `-wal` and `-shm` sidecars** to a temp dir, then delete the copies.
- **Never** run write code against real credentials. It writes to a real calendar with `sendUpdates=all`.
- **Do not mutate away any guard in front of `load_config` or `access_token_for`.** Prove ordering by **widening** (`if x || true`) or by mutating only the `bail!` literal.
- **Never** `{:?}`-log, print or interpolate a token string.
- `sqlx::query`/`query_as`/`query_scalar` only — **never** the `query!` macros.
- Svelte 5 runes only: no `export let`, no `$:`. **Never** `{@html}`.
- No hardcoded hex in app source beyond the existing `#e2564a`.
- **No live network calls to Google in tests** — wiremock (Rust), harness stubs (UI).
- Playwright specs depending on "now" must call `page.clock.setFixedTime`.
- **Every test must be shown to fail against deliberately broken code, with the mutation asserted present on disk (`grep -F`, as its own statement) before the suite runs.** Revert with a targeted `Edit` — never `git checkout -- <file>`, never `perl`.
- **Verify gates by exit code**, output to a fresh directory.

## What must not regress

- `an_all_day_create_resolves_against_the_calendars_own_timezone_not_the_authoring_one` — Plan 5 Task 5 shipped a bug in exactly this shape.
- An untouched field sends nothing. The `recurrence` three-state and the times trigger both depend on it.
- `occurrenceStartMs` is the clicked block's own `start_ms`, never `detail.start_ms`. **Two** popover instances rely on it.
- Plan 5's anchoring rule: `after`'s times reach the target as the **shift** the user made.

## File Structure

| File | Responsibility |
| --- | --- |
| `src-tauri/src/write.rs` | `EventFields`/`EventInput` carry a date for all-day; `event_time_json` splits |
| `src-tauri/src/events.rs` | `EventDetail` carries all-day dates; `edit_patch_body`'s before-side |
| `ui/src/lib/eventform.ts` | `valueFromDetail` reads the server's dates; `toEventInput` passes untouched times through |
| `ui/src/lib/eventdetail.ts` | the TS mirror |
| `ui/tests/eventform.spec.ts` | the four characterisation specs, **inverted** |

---

### Task 1: `When` — one type for "when an event happens"

**Files:**
- Modify: `src-tauri/src/write.rs`

**Interfaces:**
- Produces:
  ```rust
  pub(crate) enum When {
      Timed { start_ms: i64, end_ms: i64 },
      /// Both `yyyy-mm-dd`. `end` is **exclusive**, as Google sends it.
      AllDay { start_date: String, end_date: String },
  }
  pub(crate) fn when_json(when: &When, tz: &str) -> (Value, Value)
  ```

The point of the enum: an all-day event has no instant and no zone, so the type stops anyone supplying one. `tz` is used **only** by the `Timed` arm.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn an_all_day_when_needs_no_zone_and_sends_bare_dates() {
    let w = When::AllDay { start_date: "2026-08-10".into(), end_date: "2026-08-11".into() };
    // The zone is deliberately absurd: an all-day event must not consult it.
    let (start, end) = when_json(&w, "Not/AZone");
    assert_eq!(start, serde_json::json!({ "date": "2026-08-10" }));
    assert_eq!(end, serde_json::json!({ "date": "2026-08-11" }));
    assert!(start.get("dateTime").is_none());
    assert!(start.get("timeZone").is_none());
}

#[test]
fn a_timed_when_sends_datetime_and_zone() {
    let w = When::Timed { start_ms: 1_785_398_400_000, end_ms: 1_785_402_000_000 };
    let (start, end) = when_json(&w, "Europe/Sofia");
    assert!(start["dateTime"].is_string());
    assert_eq!(start["timeZone"], "Europe/Sofia");
    assert!(start.get("date").is_none());
    assert!(end["dateTime"].is_string());
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p omacal when_ 2>&1`. Expected: FAIL, `When` not found.

- [ ] **Step 3: Implement**

```rust
/// When an event happens.
///
/// An all-day event has **no instant and no zone** — Google models it as a bare
/// `date`, and so does the store once `omacal_sync::resolve` has read it. The
/// enum exists so nobody can supply a zone for one: the previous shape took
/// `(ms, is_all_day, tz)` and the two sides of the boundary converted that date
/// to an instant in *different* zones, which moved events nobody moved.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum When {
    Timed { start_ms: i64, end_ms: i64 },
    /// Both `yyyy-mm-dd`. `end_date` is **exclusive** — the day after the last
    /// one — matching Google's wire format and the store's `end_utc`.
    AllDay { start_date: String, end_date: String },
}

/// Google's `start` and `end` objects. `tz` is read only by the timed arm.
pub(crate) fn when_json(when: &When, tz: &str) -> (Value, Value) {
    match when {
        When::AllDay { start_date, end_date } => (
            json!({ "date": start_date }),
            json!({ "date": end_date }),
        ),
        When::Timed { start_ms, end_ms } => (
            json!({ "dateTime": omacal_sync::to_rfc3339(*start_ms), "timeZone": tz }),
            json!({ "dateTime": omacal_sync::to_rfc3339(*end_ms),   "timeZone": tz }),
        ),
    }
}
```

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p omacal when_`. Expected: PASS.

- [ ] **Step 5: Mutation-check**

Give the `AllDay` arm a `"timeZone"` key. Assert present with `grep -F`, run, confirm `an_all_day_when_needs_no_zone_and_sends_bare_dates` FAILS. Revert with a targeted `Edit`.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/write.rs && git commit -m "feat(write): one type for when an event happens"
```

---

### Task 2: `EventFields` and `EventInput` carry `When`

**Files:**
- Modify: `src-tauri/src/write.rs`

**Interfaces:**
- Consumes: `When`, `when_json` (Task 1)
- Produces: `EventFields.when: When` replacing `start_ms`/`end_ms`/`is_all_day`; `EventInput.when: WhenInput` (a `#[serde(tag = "kind", rename_all = "camelCase")]` enum with `Timed`/`AllDay` variants); `event_time_json` **deleted**, all callers on `when_json`.

`changed_fields`' times trigger becomes `before.when != after.when || before.tz != after.tz` — one comparison, and an all-day date compares as a string so no instant is involved.

> **Implementer:** `event_time_json` has callers in `events.rs` (`create_via_client`, `edit_patch_body`, `split_series`). Find them all before you start — `grep -rn event_time_json src-tauri/`. Do **not** leave a compatibility shim; the point of this task is that the old shape becomes unrepresentable.

- [ ] **Step 1: Write the failing tests**

```rust
/// The property the whole plan exists for: an all-day date that nobody edited
/// compares equal on both sides, so no `start`/`end` is sent at all.
#[test]
fn an_untouched_all_day_date_produces_an_empty_body() {
    let before = all_day_fields("2026-08-10", "2026-08-11");
    let after = all_day_fields("2026-08-10", "2026-08-11");
    assert_eq!(changed_fields(&before, &after), serde_json::json!({}));
}

#[test]
fn a_changed_all_day_date_sends_both_dates() {
    let before = all_day_fields("2026-08-10", "2026-08-11");
    let after = all_day_fields("2026-08-12", "2026-08-13");
    let body = changed_fields(&before, &after);
    assert_eq!(body["start"], serde_json::json!({ "date": "2026-08-12" }));
    assert_eq!(body["end"], serde_json::json!({ "date": "2026-08-13" }));
}

/// Switching an event between all-day and timed is a real change even when the
/// day is the same, and the two variants can never compare equal.
#[test]
fn changing_between_all_day_and_timed_always_sends_times() {
    let before = all_day_fields("2026-08-10", "2026-08-11");
    let mut after = before.clone();
    after.when = When::Timed { start_ms: 1_785_398_400_000, end_ms: 1_785_402_000_000 };
    let body = changed_fields(&before, &after);
    assert!(body.get("start").is_some(), "body was {body}");
    assert!(body["start"]["dateTime"].is_string());
}
```

> **Implementer:** write `all_day_fields(start, end)` in the test module alongside the existing `base()`.

- [ ] **Step 2: Run to verify they fail**

- [ ] **Step 3: Implement**

Replace the three fields with `when: When` on `EventFields`. On `EventInput`:

```rust
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum WhenInput {
    Timed { start_ms: i64, end_ms: i64 },
    AllDay { start_date: String, end_date: String },
}
```

`#[serde(tag = "kind")]` deliberately: the UI sends `{"kind":"allDay","startDate":"…","endDate":"…"}`, so a malformed payload fails to deserialize rather than silently defaulting.

- [ ] **Step 4: Run to verify they pass**

- [ ] **Step 5: Mutation-check**

Make the times trigger ignore `when` (compare only `tz`). Assert present, run, confirm `a_changed_all_day_date_sends_both_dates` FAILS. Revert. Then make `When`'s `PartialEq` compare only the variant (a manual impl ignoring the fields) and confirm the same test FAILS.

- [ ] **Step 6: Commit**

---

### Task 3: `EventDetail` carries the all-day dates

This is the **display** half. The form currently derives an all-day date with `dateOf(start_utc)` in the browser's zone; east of the calendar's zone that renders the previous day, before anybody presses Save.

**Files:**
- Modify: `src-tauri/src/events.rs`, `ui/src/lib/eventdetail.ts`

**Interfaces:**
- Produces: `EventDetail.start_date: Option<String>`, `EventDetail.end_date: Option<String>` — `Some` only when `is_all_day`, both `yyyy-mm-dd`, `end_date` **inclusive** (the day the user would point at), derived in the **calendar's** timezone.

> **Implementer:** `event_detail_impl` currently reads `omacal_store::event_by_id`, which does **not** return the calendar's timezone. `calendar_for_write` does (4-tuple, added in Plan 5). Decide whether to widen `event_by_id` or make a second lookup, and say which and why — `event_detail` is on the popover-open path, so a second query there is a real cost.

- [ ] **Step 1: Write the failing test**

```rust
/// The display half of the boundary defect. A trip stored as midnight in
/// New York, read from a Sofia machine, must still report 10 August — the UI
/// must never derive this date itself.
#[tokio::test]
async fn an_all_day_detail_reports_dates_in_the_calendars_zone() {
    // seed: calendar timezone America/New_York, an all-day event on 2026-08-10
    // (inclusive last day 2026-08-10, exclusive end 2026-08-11)
    // assert d.start_date == Some("2026-08-10") and d.end_date == Some("2026-08-10")
}

#[tokio::test]
async fn a_timed_detail_reports_no_dates() {
    // assert both are None
}
```

> **Implementer:** the seeded instants must be genuine midnights in `America/New_York`, not in UTC — compute them, and assert your own fixture (e.g. that the stored `start_utc` is not a UTC midnight) so it cannot quietly stop diverging.

- [ ] **Steps 2–4: Run failing, implement, run passing**

- [ ] **Step 5: Mutation-check**

Derive the dates in UTC instead of the calendar's zone. Assert present, run, confirm the first test FAILS. Revert.

- [ ] **Step 6: Commit**

---

### Task 4: the UI stops deriving all-day dates

**Files:**
- Modify: `ui/src/lib/eventform.ts`

`valueFromDetail` takes `detail.start_date`/`detail.end_date` when `is_all_day`, instead of `dateOf(startMs)` / `dateOf(endMs - DAY_MS/2)`. `instantsOf` and `toEventInput` build a `WhenInput` — `AllDay` straight from `value.date`/`value.endDate` (converting inclusive→exclusive **once**, with `addDays`, no instant), `Timed` as today.

`endAfterStart` must keep working for both: compare dates as strings for all-day, instants for timed.

- [ ] **Step 1: Write the failing specs**

In `ui/tests/eventform.spec.ts`, and **invert two of the four characterisation specs** — the all-day ones. They currently assert the wrong value on purpose; they must now assert the right one.

```ts
test('an all-day value round-trips its dates without touching an instant', () => {
  // valueFromDetail with start_date/end_date set, then toEventInput,
  // asserts when.kind === 'allDay' and the dates come back unchanged,
  // in a describe whose timezoneId differs from the fixture's calendar zone.
});

test('an all-day end date converts inclusive to exclusive exactly once', () => {
  // display shows the inclusive last day; the input carries the exclusive one.
});
```

- [ ] **Steps 2–4: Run failing, implement, run passing**

- [ ] **Step 5: Mutation-check**

Make `valueFromDetail` fall back to `dateOf(startMs)` for all-day. Assert present, run, confirm the round-trip spec FAILS **in the differing-zone describe** and would have passed in a UTC one. Revert.

- [ ] **Step 6: Commit**

---

### Task 5: an untouched time passes its instant through

This is the **drift** half, and it is smaller than it looks. `toEventInput` already receives `initial`. When the civil fields are untouched, it must send the **original instants** rather than re-deriving them through `toMs` — which is where a repeated hour loses an hour and a sub-minute start loses its seconds.

**Files:**
- Modify: `ui/src/lib/eventform.ts`

**Interfaces:**
- `toEventInput(value, initial)` gains: if `value.date === initial.date && value.start === initial.start && value.endDate === initial.endDate && value.end === initial.end && value.isAllDay === initial.isAllDay`, carry `initial`'s source instants through unchanged.

> That requires `EventFormValue` to *have* the source instants. Add `sourceStart?: number` / `sourceEnd?: number`, set by `valueFromDetail` from the `startMs`/`endMs` it is already given, absent on a create. **Implementer:** if you find a cleaner shape, take it and say why — but the property is what matters: an untouched time must not be re-derived.

- [ ] **Step 1: Write the failing specs**

**Invert the other two characterisation specs** (the repeated-hour edit and the seconds one) so they assert zero drift, and add:

```ts
test('editing only the title of an event in a repeated hour sends no times', () => {
  // Europe/Sofia, 25 Oct 2026, a start in the second pass.
  // Change only the title. Assert the input's when equals the source instants
  // exactly — drift 0, not "close".
});

test('a time the user did edit is re-derived, not passed through', () => {
  // otherwise the pass-through would freeze the time against real edits.
});
```

- [ ] **Steps 2–4: Run failing, implement, run passing**

- [ ] **Step 5: Mutation-check**

Remove the untouched check so times are always re-derived. Assert present, run, confirm the repeated-hour spec FAILS with a −3,600,000 ms drift. Revert. Then make the check always fire (never re-derive) and confirm the second spec FAILS.

- [ ] **Step 6: Commit**

---

### Task 6: the skipped midnight, and the sweep

**Files:**
- Modify: `ui/src/lib/eventform.ts`, and whatever the sweep turns up

> **This section was rewritten after the fact.** What it originally asked for
> was stale by the time the task ran, in two ways that mattered: its sweep
> grepped for `instantsOf`, which Task 4 had **deleted**, and its greps could
> not have found the one live survivor — a `toLocaleDateString` in
> `EventPopover`. Its fix, "move the end by the same civil span", was also not
> the one that shipped. What follows is what Task 6 actually did.

Four things.

**1. Amend design §3.** §3 prescribed "`toMs` gains explicit handling: ambiguous
(repeated hour) — resolve to the **first** pass". That mechanism is wrong, and
Task 5's review established why: applied to an **untouched** second-pass start,
resolving to the first pass *is* the −1 hour drift of defect 2.2, made
deliberate. It would not have closed the defect at all. What shipped is the
**pass-through** — an untouched time is sent as the instant it was read off —
which satisfies §3's stated *rule* exactly and needs no resolver. §3 now names
the pass-through and says plainly why resolve-to-first-pass is wrong, so a
future reader following the old text cannot reintroduce the defect. §3's
nonexistent-time clause is likewise amended to describe what `Date`
normalisation actually does rather than an unimplemented ideal.

**2. The skipped midnight — closed without a resolver.** `blankValue` moving its
default onto a chosen day built `date` from the day and `start` from a clock on
a *different* day, so the pair was read off no instant and on America/Santiago
6 Sep 2026 named none. It now **re-anchors**: `blankValueAt(toMs(date, start))`,
which rebuilds the whole value from the instant the moved pair names. On that
day it gives 01:30–02:00, span +30, saveable. That is *not* §3's letter ("the
first valid instant" is 01:00); normalisation lands on 01:30, and §3 is amended
to say so rather than the code contorted to match it. The Santiago
characterisation spec — the last one standing — is **inverted**.

**3. The skipped-hour typed edit — pinned, with the reason on record.** A case
neither the plan nor Task 5 had: in Santiago on 6 Sep, typing `00:30` and
`01:30` leaves both strings on screen while `whenOf` answers 01:30 for both —
span 0, Save dead, no field visibly wrong. **Characterised, not fixed.** A
create is the *app* choosing a time and may be re-anchored silently; a typed
time is not the app's to move, and this branch already ruled that an incoherent
pair is refused honestly rather than repaired by dragging an untouched field.
Closing it needs the form to *say* the time does not exist — a form-level
affordance, not a boundary conversion. Measured rather than argued: a mutation
making `toMs` refuse a civil pair that does not read back as itself fails the
characterisation spec **and** the create above, so the cheap repair for one
breaks the other.

**4. The sweep**, with `toLocale*` added — which is the point, since the
original grep would have missed the only live finding.
`grep -rn "toMs(\|dateOf(\|timeOf(\|toLocaleDateString\|toLocaleTimeString" ui/src`,
every hit checked on an all-day or occurrence path. One survivor:
`EventPopover`'s `when` line rendered `detail.start_ms` through
`toLocaleDateString` in the **browser's** zone — a browser-zone reading of an
all-day instant *and* the **master's** row rather than the `occurrenceStartMs`
the component already had. Before Tasks 3–4 the popover and the form were wrong
together; afterwards they disagreed on screen. Fixed on both arms, with
`occurrenceEndMs` added as a required prop so the clock comes from the clicked
block too. Every other hit resolved or recorded with a reason in the task
report.

- [x] **Step 1: Write the failing specs**

Five, and the Santiago inversion. The two that carry the sweep are new
`EventPopover` fixtures — the first here able to catch that line at all, since
every existing one has `occurrenceStartMs === detail.start_ms`.

- [x] **Steps 2–6: as above**

---

## Definition of Done

- [ ] `cargo test --workspace` ≥ 380 passed, 0 failed — **by exit code**
- [ ] `npm --prefix ui run test:ui` ≥ 460 passed — **by exit code**
- [ ] `npm --prefix ui run check` — 0 errors, 0 warnings
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` — clean
- [ ] **All four Plan 5 characterisation specs inverted**, each now asserting the correct value, and each shown to fail against the pre-fix implementation — in the event **five**, the fifth being a Rust one Task 2's type change made unrepresentable. One characterisation spec remains and is **not** one of them: the skipped-hour *typed edit*, found while closing the create in Task 6 (see that section).
- [ ] `an_all_day_create_resolves_against_the_calendars_own_timezone_not_the_authoring_one` still binds
- [ ] At least one fixture has the calendar's zone differing from the browser's — the suite was structurally blind to all three defects because none did
- [ ] `event_time_json` is gone, with no compatibility shim

> **On the bars:** estimates. If the honest number is lower, report the real one — do **not** pad the suite and do **not** lower the bar. Both have been tried on this project and both were caught.
