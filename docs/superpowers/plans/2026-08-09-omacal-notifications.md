# Notifications — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** omacal tells you about a meeting before it starts, on Omarchy, without the window being open.

**Spec:** [`2026-08-09-omacal-notifications-design.md`](../specs/2026-08-09-omacal-notifications-design.md), which extends §6 of the original design.

**Base:** `main` @ `8f02e39` — 417 Rust tests, 604 UI tests.

## Global Constraints

- **Never** modify `~/Library/Application Support/com.omacal.app/omacal.db` or `~/.config/omacal/config.toml` (three files each — `-wal` and `-shm` too).
- `sqlx::query`/`query_as`/`query_scalar` only — **never** the `query!` macros.
- **Never** `{:?}`-log or interpolate a `Tokens` value or any token string.
- **No live network calls in tests.** wiremock for Rust, harness stubs for UI.
- **No test may sleep to observe the scheduler.** `now` is a parameter, never a read.
- **Demo mode posts no notifications at all** — a fourth enforcement point beside the separate DB, `demo_sync_guard`, and `should_sync`/`may_sync`.
- Every test shown to fail against broken code, with the mutation **asserted on disk** (`grep -F`, its own statement, with a count or line match) before the suite runs. Revert with a targeted `Edit` — never `git checkout -- <file>`, never `perl`.
- **Verify gates by exit code.** `cargo test --workspace` (417), `npm --prefix ui run test:ui` (604), `npm --prefix ui run check`, `cargo clippy --workspace --all-targets -- -D warnings`.

## The one thing that must not break

`crates/omacal-google/src/client.rs:89` — *"Every parameter here must stay byte-identical across incremental calls or Google invalidates the sync token."*

There is **no `fields=` mask** on `list_events`, so Google already returns `reminders` on every event and `defaultReminders` on every calendar list entry. **Task 1 adds no request parameter.** Adding one to "ask for" reminders would silently invalidate every stored sync token and force a full resync on every calendar — the most expensive possible way to get data that was already arriving.

---

### Task 1: reminders arrive and are stored

**Files:** `crates/omacal-google/src/model.rs`, `crates/omacal-store/migrations/`, `crates/omacal-sync/src/`

`Event` gains `reminders: Reminders` and `Calendar` gains `default_reminders: Vec<Reminder>`:

```rust
pub struct Reminder { pub method: String, pub minutes: i64 }
pub struct Reminders { pub use_default: bool, pub overrides: Vec<Reminder> }
```

`reminders_json TEXT` already exists on `events` (`0001_init.sql`) and is unused — populate it. Calendars need a new column for their defaults; add a migration.

- [ ] **Step 1:** a `model.rs` test deserialising a real Google event payload with `reminders: {useDefault: false, overrides: [{method: "popup", minutes: 10}]}`, and one with `{useDefault: true}`. Include an event with **no** `reminders` key at all — cancelled tombstones carry almost nothing, and a missing key must not fail the parse.
- [ ] **Step 2:** run, confirm it fails.
- [ ] **Step 3:** implement, including the calendar migration.
- [ ] **Step 4:** run, confirm it passes.
- [ ] **Step 5:** a wiremock sync test proving a synced event's `reminders_json` round-trips, and that `list_events`'s query parameters are **unchanged** — assert the recorded request's query string explicitly, so a future `fields=` cannot be added without failing.
- [ ] **Step 6:** mutate the parse to drop `overrides`; assert on disk; confirm failure; revert; commit.

### Task 2: `due_reminders`, a pure function

**Files:** new module in `crates/omacal-core/src/`

```rust
pub fn due_reminders(
    events: &[ScheduledEvent],   // occurrence-expanded, with calendar zone + selected flag
    fired: &HashSet<FiredKey>,
    now_ms: i64,
    horizon_ms: i64,
) -> Vec<Due>
```

No clock, no I/O, no database. Every rule from the spec lives here:

- fire-time is `occurrence_start - minutes`, from `overrides` when `use_default` is false, else the calendar's defaults
- **only `method == "popup"` fires.** `email` is Google's to send; firing it locally would double it
- all-day start is **midnight in the calendar's own zone** — reuse `write.rs::date_in_zone`, do not write a second derivation
- only calendars with `selected = true`
- already-fired keys excluded
- a reminder whose time has passed fires only while `now < occurrence_end`

- [ ] **Step 1:** tests, one per rule, each with a fixture that can witness *only* that rule. The all-day fixture needs a calendar zone putting midnight on a **different UTC date** (Auckland +12 against UTC) or it cannot see the rule it is named for.
- [ ] **Step 2–4:** run failing, implement, run passing.
- [ ] **Step 5:** mutate each rule separately and confirm **only** its own test fails. A mutation that fails two tests means one of them is not witnessing what it claims.
- [ ] **Step 6:** commit.

### Task 3: fired state, and the driver

**Files:** `crates/omacal-store/migrations/`, `src-tauri/src/notify_loop.rs`

```sql
CREATE TABLE fired_reminders (
  event_id      INTEGER NOT NULL,
  occurrence_ms INTEGER NOT NULL,
  minutes       INTEGER NOT NULL,
  fired_at_ms   INTEGER NOT NULL,
  PRIMARY KEY (event_id, occurrence_ms, minutes)
);
```

Keyed by occurrence so a weekly standup fires weekly, and by `minutes` so an event's 10-minute and 1-minute reminders are separate.

The driver calls `due_reminders`, posts, records, prunes rows older than the horizon, and waits for the earlier of the next fire-time or the next sync. **It is the only thing that reads a clock.**

- [ ] **Step 1:** store tests for insert/query/prune. A test proving a recorded reminder is not returned twice — that is the restart-during-a-meeting case and it is the whole reason the table exists.
- [ ] **Step 2–4:** run failing, implement, run passing.
- [ ] **Step 5:** mutate the primary key to drop `minutes` and confirm the two-reminders-on-one-event test fails.
- [ ] **Step 6:** commit.

### Task 4: the transport, behind a trait

**Files:** `src-tauri/src/notify.rs`, `src-tauri/Cargo.toml`

```rust
pub trait Notifier: Send + Sync {
    fn post(&self, n: &Notification) -> Result<(), NotifyError>;
}
```

Linux: `notify-rust` → D-Bus, with *Join* (when a conference URI exists) and *Snooze 5m*. macOS: `tauri-plugin-notification`, **allowed to fail quietly** — logged, never surfaced as an error banner, never retried into a loop.

- [ ] **Step 1:** a recording fake, and driver tests asserting what would be posted for a given clock — title, body, whether Join is present. No test posts a real notification.
- [ ] **Step 2–4:** run failing, implement, run passing.
- [ ] **Step 5:** mutate the Join condition to always-present; confirm the no-conference test fails.
- [ ] **Step 6:** commit.

### Task 5: tray, autostart, and the demo guard

**Files:** `src-tauri/src/lib.rs`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`

Tray with Open/Sync now/Quit. `tauri-plugin-autostart`. **Closing the window hides it** — quit is explicit from the tray, because a closed window that stopped firing reminders would be a bug.

- [ ] **Step 1:** a test that demo mode posts **nothing**, through the same path a real run posts something — a demo test that never reaches the notifier proves nothing. This is the fourth demo enforcement point and it is the one worth the most care.
- [ ] **Step 2–4:** run failing, implement, run passing.
- [ ] **Step 5:** mutate the demo guard to a no-op; confirm the test fails. Then confirm the *real* path still posts, so the guard is not simply disabling everything.
- [ ] **Step 6:** commit.

---

## Definition of Done

- [ ] `cargo test --workspace` ≥ 450, 0 failed — **by exit code**
- [ ] `npm --prefix ui run test:ui` ≥ 604 — **by exit code**
- [ ] `check` 0 errors; `clippy` clean
- [ ] No test sleeps, and no test posts a real notification
- [ ] `list_events`'s query parameters provably unchanged
- [ ] Demo mode posts nothing, witnessed through the path that otherwise posts
- [ ] A reminder is not re-posted after a restart

> **On the bars:** estimates. If the honest number is lower, report the real one — do not pad the suite and do not lower the bar. Both have been tried on this project and both were caught.
