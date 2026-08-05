# omacal Plan 1b: Operability, Live Sync & UI Test Suite

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the merged Plan 1 branch — correct, tested, and completely unusable — into an app you can launch on macOS, look at, sign into, and leave running while it syncs.

**Architecture:** Plan 1 built four library crates and a week grid, but nothing in the UI calls the `sign_in`/`sync_now` commands and no background timer exists. This plan adds the operability layer (status command, header controls, tick loop, theme watcher), a demo-data mode so the app can be *seen* without Google credentials, and the committed UI test suite Plan 1 never had.

**Tech Stack:** Rust (Tauri v2, tokio, notify, jiff, sqlx), TypeScript + Svelte 5, Playwright (WebKit + Chromium).

**Spec:** `docs/superpowers/specs/2026-08-05-omacal-design.md`
**Predecessor:** `docs/superpowers/plans/2026-08-05-omacal-m0-m1-foundation.md` (complete, merged to `main`)

## Why this plan exists

Plan 1's final whole-branch review found that every task passed its own brief while three spec requirements fell through the gaps between them, because no task owned them:

- **§5** mandates sync "every 5 minutes (configurable), plus on window focus, plus on wake-from-sleep". No tick loop exists; `sync_now` is manual-only and unreachable.
- **§10** requires watching the theme path and repainting live on `omarchy-theme-set`. `notify` is not even a dependency.
- Nothing in `ui/` calls `sign_in` or `sync_now`. `grep sign_in ui/src/` returns zero hits.

Separately, the UI shipped with **no test suite at all** — only type-checking. The visual checks during Plan 1 were ad-hoc Playwright harnesses that implementers deleted before committing, run on Chromium rather than the WebKitGTK engine the Linux target uses.

## Global Constraints

- **Rust edition 2021**; workspace members `crates/*` and `src-tauri`. The Tauri package is `omacal`, lib target `omacal_lib`.
- **Time is `i64` epoch milliseconds** at every boundary. `chrono` remains confined to `crates/omacal-core`; `jiff` everywhere else.
- **Never `{:?}`-log, print, or interpolate a `Tokens` value or any token string.** `Tokens` has a redacting `Debug` as of Plan 1's fix wave — do not replace it with a derive.
- **The CSRF check at `src-tauri/src/lib.rs` must not be removed or weakened.** It has no automated coverage; it is held in place by review.
- **Use `sqlx::query`/`query_as`/`query_scalar` (runtime-checked), never the `query!` macros.**
- **Day-boundary arithmetic always goes through `day_boundaries`**, never `n * 86_400_000`.
- **The app must start even if the theme cannot be parsed** — fall back to the built-in dark palette and log a warning.
- **No live network calls in tests.**
- **Demo mode must never write to the real database.** It uses a separate file.
- `cargo test --workspace` starts at **123** and must never regress.

---

## File Structure

| Path | Responsibility |
| --- | --- |
| `src-tauri/src/fixtures.rs` | Demo-data seeding (dev only) |
| `src-tauri/src/status.rs` | `AppStatus` type + `get_status` query |
| `src-tauri/src/sync_loop.rs` | Background tick loop, focus/wake triggers |
| `src-tauri/src/theme_watch.rs` | Filesystem watcher + `theme-changed` event |
| `src-tauri/src/lib.rs` | Wiring only — commands registered, tasks spawned |
| `ui/src/lib/Header.svelte` | Month title, navigation, sync status, sign-in |
| `ui/src/lib/status.ts` | Status/sync/sign-in bindings + event listeners |
| `ui/tests/harness/index.html` | Component mount harness for Playwright |
| `ui/tests/harness/mount.ts` | Mounts a component by query param |
| `ui/tests/fixtures.ts` | Shared synthetic payloads for tests |
| `ui/tests/*.spec.ts` | Component + visual-regression specs |
| `ui/playwright.config.ts` | WebKit + Chromium projects |
| `docs/running-on-macos.md` | Setup, credentials, demo mode, run commands |

---

### Task 1: Demo-data mode

Highest-value task in the plan: it makes the app *visible* without Google credentials, and it is the fixture every later UI test renders against. Without it, nobody can look at what Plan 1 built.

**Files:**
- Create: `src-tauri/src/fixtures.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `omacal_store::{connect, StoredEvent, upsert_event}`
- Produces:
  ```rust
  pub fn demo_mode() -> bool;                       // OMACAL_SEED_DEMO=1
  pub async fn seed_demo(pool: &SqlitePool, now_ms: i64) -> anyhow::Result<usize>;
  ```
  `seed_demo` is idempotent: it clears its own account's rows first, so repeated launches do not accumulate duplicates. Returns the number of events written.

- [ ] **Step 1: Write the failing tests**

```rust
// src-tauri/src/fixtures.rs
#[cfg(test)]
mod tests {
    use super::*;

    /// Monday 2026-08-03 00:00:00 UTC
    const MON: i64 = 1_785_715_200_000;
    const DAY: i64 = 24 * 3_600_000;

    #[tokio::test]
    async fn seeding_creates_an_account_calendars_and_events() {
        let pool = omacal_store::connect_memory().await.unwrap();
        let n = seed_demo(&pool, MON + 3 * DAY).await.unwrap();
        assert!(n >= 15, "expected a rich fixture, got {n} events");

        let accounts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(accounts, 1);

        let cals: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM calendars")
            .fetch_one(&pool).await.unwrap();
        assert!(cals >= 2, "need multiple calendars to exercise per-calendar colour");
    }

    #[tokio::test]
    async fn seeding_twice_does_not_duplicate() {
        let pool = omacal_store::connect_memory().await.unwrap();
        let first = seed_demo(&pool, MON + 3 * DAY).await.unwrap();
        let second = seed_demo(&pool, MON + 3 * DAY).await.unwrap();
        assert_eq!(first, second);

        let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(events as usize, first);
    }

    #[tokio::test]
    async fn the_fixture_exercises_every_rsvp_state() {
        let pool = omacal_store::connect_memory().await.unwrap();
        seed_demo(&pool, MON + 3 * DAY).await.unwrap();
        for state in ["accepted", "needsAction", "tentative", "declined"] {
            let n: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM events WHERE self_response = ?1")
                .bind(state).fetch_one(&pool).await.unwrap();
            assert!(n > 0, "fixture has no event in state {state}");
        }
    }

    #[tokio::test]
    async fn the_fixture_exercises_overlaps_and_all_day_spans() {
        let pool = omacal_store::connect_memory().await.unwrap();
        seed_demo(&pool, MON + 3 * DAY).await.unwrap();

        let all_day: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM events WHERE is_all_day = 1")
            .fetch_one(&pool).await.unwrap();
        assert!(all_day >= 2, "need multi-day spans for the all-day band");

        let recurring: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM events WHERE recurrence IS NOT NULL")
            .fetch_one(&pool).await.unwrap();
        assert!(recurring >= 1, "need a recurring series");

        // Two events at identical times on the same day => 50/50 column split.
        let exact_overlap: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM (
               SELECT start_utc, end_utc FROM events WHERE is_all_day = 0
               GROUP BY start_utc, end_utc HAVING COUNT(*) >= 2)")
            .fetch_one(&pool).await.unwrap();
        assert!(exact_overlap >= 1, "need an exact overlap to exercise column splitting");
    }

    #[test]
    fn demo_mode_is_off_unless_explicitly_enabled() {
        // Guard against a stray env var making a real launch load fake data.
        std::env::remove_var("OMACAL_SEED_DEMO");
        assert!(!demo_mode());
        std::env::set_var("OMACAL_SEED_DEMO", "0");
        assert!(!demo_mode());
        std::env::set_var("OMACAL_SEED_DEMO", "1");
        assert!(demo_mode());
        std::env::remove_var("OMACAL_SEED_DEMO");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p omacal fixtures`
Expected: FAIL — `cannot find function seed_demo`.

- [ ] **Step 3: Implement**

```rust
// src-tauri/src/fixtures.rs  (above the tests module)
use omacal_store::StoredEvent;
use sqlx::SqlitePool;

const DAY_MS: i64 = 24 * 3_600_000;
const MIN: i64 = 60_000;

/// The demo account's marker. Seeding removes and rewrites only this account,
/// so it can never disturb real synced data.
const DEMO_SUB: &str = "demo@omacal.local";

/// Demo mode is opt-in and explicit. It also uses a separate database file
/// (see `lib.rs`), so fake events can never reach the real store.
pub fn demo_mode() -> bool {
    std::env::var("OMACAL_SEED_DEMO").map(|v| v == "1").unwrap_or(false)
}

struct Spec {
    cal: usize,
    title: &'static str,
    location: Option<&'static str>,
    /// Day offset from Monday of the seeded week.
    day: i64,
    /// Minutes from local midnight; ignored when `all_day`.
    start_min: i64,
    dur_min: i64,
    all_day: bool,
    /// Inclusive extra days for an all-day span.
    extra_days: i64,
    response: &'static str,
    recurrence: Option<&'static str>,
}

/// Deliberately covers every visual case the week view can render:
/// all four RSVP states, each rung of the duration ladder, a 2-way exact
/// overlap, a 3-way pile, a partial overlap, multi-day all-day spans
/// including one that begins before the week, and a recurring series.
fn specs() -> Vec<Spec> {
    let s = |cal, title, location, day, start_min, dur_min, response, recurrence| Spec {
        cal, title, location, day, start_min, dur_min,
        all_day: false, extra_days: 0, response, recurrence,
    };
    vec![
        // Recurring daily standup — 30 min, title-only rung.
        s(0, "Standup", Some("Meet"), 0, 9 * 60, 30, "accepted", Some("RRULE:FREQ=DAILY;COUNT=5")),
        // Monday
        s(0, "Excitel weekly", Some("Meet"), 0, 11 * 60, 60, "accepted", None),
        s(1, "1:1 Rahul", None, 0, 15 * 60, 30, "accepted", None),
        // Tuesday — 90 min rung (time gets its own line)
        s(0, "NetSense demo", Some("Zoom"), 1, 13 * 60, 90, "accepted", None),
        // Wednesday — three-way pile
        s(0, "Board prep", Some("Room 4A"), 2, 10 * 60, 120, "accepted", None),
        s(1, "Vendor sync", Some("Meet"), 2, 11 * 60, 60, "accepted", None),
        s(1, "Legal review", None, 2, 11 * 60 + 30, 60, "tentative", None),
        s(0, "Deep work", Some("Focus"), 2, 14 * 60, 180, "accepted", None),
        // Thursday — exact overlap, 50/50 split
        s(0, "Ops review", Some("Meet"), 3, 10 * 60, 60, "accepted", None),
        s(1, "Investors", Some("Zoom"), 3, 10 * 60, 60, "needsAction", None),
        s(0, "All hands", Some("Meet"), 3, 16 * 60, 60, "declined", None),
        // Friday — partial overlap
        s(0, "Retro", Some("Meet"), 4, 11 * 60, 60, "accepted", None),
        s(1, "Interview", Some("Room 2"), 4, 11 * 60 + 30, 60, "needsAction", None),
        s(1, "Gym", None, 4, 17 * 60, 60, "accepted", None),
        // Short event — 15 min, proves fill-based RSVP survives at minimum height
        s(0, "Sync w/ Ivan", None, 1, 16 * 60, 15, "needsAction", None),
        // All-day spans
        Spec { cal: 1, title: "Rahul on leave", location: None, day: 0, start_min: 0,
               dur_min: 0, all_day: true, extra_days: 2, response: "accepted", recurrence: None },
        Spec { cal: 1, title: "Sofia trip", location: None, day: 5, start_min: 0,
               dur_min: 0, all_day: true, extra_days: 1, response: "accepted", recurrence: None },
        // Begins before the week — exercises the `cont_left` flat/dashed edge
        Spec { cal: 0, title: "Q3 planning", location: None, day: -2, start_min: 0,
               dur_min: 0, all_day: true, extra_days: 3, response: "accepted", recurrence: None },
    ]
}

/// Seeds a realistic week around `now_ms`. Idempotent: the demo account and
/// everything cascading from it is removed first.
pub async fn seed_demo(pool: &SqlitePool, now_ms: i64) -> anyhow::Result<usize> {
    sqlx::query("DELETE FROM accounts WHERE google_sub = ?1")
        .bind(DEMO_SUB)
        .execute(pool)
        .await?;

    let account_id: i64 = sqlx::query_scalar(
        "INSERT INTO accounts (google_sub, email, display_name, created_at)
         VALUES (?1, ?1, 'Demo', ?2) RETURNING id",
    )
    .bind(DEMO_SUB)
    .bind(now_ms)
    .fetch_one(pool)
    .await?;

    // Two calendars with distinct colours, so per-calendar colour is visible.
    let mut cal_ids = Vec::new();
    for (gid, summary, colour, primary) in [
        ("demo-work", "Work", "#5b8def", 1),
        ("demo-personal", "Personal", "#4cc38a", 0),
    ] {
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO calendars
                 (account_id, google_id, summary, color_hex, timezone, access_role, is_primary)
             VALUES (?1, ?2, ?3, ?4, 'UTC', 'owner', ?5) RETURNING id",
        )
        .bind(account_id)
        .bind(gid)
        .bind(summary)
        .bind(colour)
        .bind(primary)
        .fetch_one(pool)
        .await?;
        cal_ids.push(id);
    }

    // Monday 00:00 UTC of the week containing `now_ms`.
    let week_start = {
        let days = now_ms.div_euclid(DAY_MS);
        // 1970-01-01 was a Thursday; shift so 0 == Monday.
        let dow = (days + 3).rem_euclid(7);
        (days - dow) * DAY_MS
    };

    let mut written = 0usize;
    for (i, sp) in specs().iter().enumerate() {
        let day_start = week_start + sp.day * DAY_MS;
        let (start, end) = if sp.all_day {
            (day_start, day_start + (sp.extra_days + 1) * DAY_MS)
        } else {
            let s = day_start + sp.start_min * MIN;
            (s, s + sp.dur_min * MIN)
        };

        omacal_store::upsert_event(
            pool,
            &StoredEvent {
                id: 0,
                calendar_id: cal_ids[sp.cal],
                google_id: format!("demo-{i}"),
                summary: Some(sp.title.to_string()),
                location: sp.location.map(str::to_string),
                start_utc: start,
                end_utc: end,
                start_tz: "UTC".into(),
                end_tz: "UTC".into(),
                is_all_day: sp.all_day,
                recurrence: sp.recurrence.map(str::to_string),
                recurring_event_id: None,
                original_start_utc: None,
                status: "confirmed".into(),
                self_response: Some(sp.response.to_string()),
                conference_uri: None,
                color_hex: None,
            },
        )
        .await?;
        written += 1;
    }

    Ok(written)
}
```

> **Note on `StoredEvent` fields.** Plan 1's fix wave added `recurring_event_id`, `original_start_utc` and `color_hex`. If the struct in `crates/omacal-store/src/events.rs` differs from the literal above, match the real struct — do not change the struct to match this plan.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p omacal fixtures`
Expected: 5 passed.

- [ ] **Step 5: Wire demo mode into startup**

In `src-tauri/src/lib.rs`, add `mod fixtures;` and change the `setup` closure so demo mode uses a **separate database file** and seeds it:

```rust
let dir = app.path().app_data_dir()?;
std::fs::create_dir_all(&dir)?;

let db_name = if fixtures::demo_mode() { "omacal-demo.db" } else { "omacal.db" };
let url = format!("sqlite://{}", dir.join(db_name).display());
let pool = tauri::async_runtime::block_on(omacal_store::connect(&url))?;

if fixtures::demo_mode() {
    let now = now_ms();
    let seeded = tauri::async_runtime::block_on(fixtures::seed_demo(&pool, now))?;
    tracing::warn!(seeded, db = db_name, "DEMO MODE — synthetic data, not your calendar");
}

app.manage(AppState { pool });
```

- [ ] **Step 6: Verify by launching**

Run: `OMACAL_SEED_DEMO=1 cargo tauri dev`
Expected: a populated week — overlapping blocks on Wednesday and Thursday, a dashed unanswered invite, a hatched tentative block, a struck-through declined one, all-day chips spanning several days with a dashed left edge on the one that began last week, and two distinct calendar colours.

Confirm the real database is untouched:

```bash
ls -la "$HOME/Library/Application Support/com.omacal.app/"
```
Expected: `omacal-demo.db` exists; `omacal.db` is absent or unchanged.

- [ ] **Step 7: Commit**

```bash
cargo test --workspace
git add src-tauri
git commit -m "feat(dev): demo-data mode with a separate database"
```

---

### Task 2: Playwright component + visual-regression suite

Plan 1 shipped the UI with no tests. Its visual checks were temporary harnesses that were deleted, run on Chromium rather than the WebKit family the Linux target uses. This makes them permanent, committed, and runnable on both engines.

**Files:**
- Create: `ui/playwright.config.ts`, `ui/tests/harness/index.html`, `ui/tests/harness/mount.ts`, `ui/tests/fixtures.ts`, `ui/tests/components.spec.ts`, `ui/tsconfig.test.json`
- Modify: `ui/package.json`, `ui/tsconfig.json`, `.gitignore`

**Interfaces:**
- Consumes: `WeekGrid`, `EventBlock`, `AllDayBand`, and the types in `ui/src/lib/api.ts`
- Produces: `npm --prefix ui run test:ui` (headless, both engines) and committed snapshots under `ui/tests/components.spec.ts-snapshots/`

- [ ] **Step 1: Install and configure Playwright**

```bash
npm --prefix ui i -D @playwright/test
npx --prefix ui playwright install webkit chromium
```

```ts
// ui/playwright.config.ts
import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: './tests',
  // Snapshots are the point of this suite; a stale one must fail, not silently update.
  updateSnapshots: 'missing',
  expect: { toHaveScreenshot: { maxDiffPixelRatio: 0.01 } },
  use: { baseURL: 'http://localhost:5199' },
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
```

- [ ] **Step 2: Build the mount harness**

```html
<!-- ui/tests/harness/index.html -->
<!doctype html>
<html>
  <head><meta charset="utf-8" /><title>omacal harness</title></head>
  <body style="margin:0">
    <div id="app"></div>
    <script type="module" src="/tests/harness/mount.ts"></script>
  </body>
</html>
```

```ts
// ui/tests/harness/mount.ts
import { mount } from 'svelte';
import WeekGrid from '../../src/lib/WeekGrid.svelte';
import EventBlock from '../../src/lib/EventBlock.svelte';
import AllDayBand from '../../src/lib/AllDayBand.svelte';
import { FIXTURES } from '../fixtures';

// Palette normally arrives from the Rust get_palette command; the harness
// applies the same fallback_dark values so snapshots are deterministic.
const PALETTE: Record<string, string> = {
  '--bg': '#17171a', '--surface': '#1e1e22', '--text': '#e8e8ea',
  '--muted': '#8a8a90', '--accent': '#5b8def',
  '--hairline': 'rgba(255,255,255,.055)',
  '--hour-rule': 'rgba(255,255,255,.035)',
  '--today-tint': 'rgba(255,255,255,.028)',
};
for (const [k, v] of Object.entries(PALETTE)) {
  document.documentElement.style.setProperty(k, v);
}
document.body.style.background = PALETTE['--bg'];
document.body.style.color = PALETTE['--text'];

const params = new URLSearchParams(location.search);
const name = params.get('c') ?? 'WeekGrid';
const fixture = params.get('f') ?? 'default';

const COMPONENTS: Record<string, any> = { WeekGrid, EventBlock, AllDayBand };
const target = document.getElementById('app')!;

const props = FIXTURES[name]?.[fixture];
if (!props) {
  target.textContent = `no fixture ${name}/${fixture}`;
} else {
  // EventBlock is absolutely positioned; give it a sized relative parent.
  if (name === 'EventBlock') {
    target.style.position = 'relative';
    target.style.height = '480px';
    target.style.width = '220px';
  }
  mount(COMPONENTS[name], { target, props });
}
```

- [ ] **Step 3: Write the shared fixtures**

```ts
// ui/tests/fixtures.ts
import type { UiEvent, Placed, Lane, WeekPayload } from '../src/lib/api';

const H = 3_600_000;
/** Monday 2026-08-03 00:00:00 UTC — fixed so snapshots never drift. */
export const MON = 1_785_715_200_000;

const ev = (o: Partial<UiEvent> & { title: string; start_ms: number; end_ms: number }): UiEvent => ({
  id: Math.floor(o.start_ms / 1000),
  location: null,
  color: '#5b8def',
  response: 'accepted',
  is_all_day: false,
  ...o,
});

const placed = (top: number, height: number, column = 0, columns = 1, idx = 0): Placed =>
  ({ idx, column, columns, top, height });

const day = (offset: number, events: UiEvent[], p: Placed[]) => ({
  start_ms: MON + offset * 24 * H,
  end_ms: MON + (offset + 1) * 24 * H,
  events,
  placed: p,
});

const emptyWeek = (): WeekPayload => ({
  days: Array.from({ length: 7 }, (_, i) => day(i, [], [])),
  all_day: [],
  all_day_events: [],
  overflow: [],
});

const populatedWeek = (): WeekPayload => {
  const w = emptyWeek();
  // Monday: a single 60-minute meeting.
  w.days[0] = day(0, [ev({ title: 'Excitel weekly', location: 'Meet',
    start_ms: MON + 11 * H, end_ms: MON + 12 * H })], [placed(11 / 24, 1 / 24)]);
  // Thursday: two meetings at identical times => 50/50 split.
  const th = MON + 3 * 24 * H;
  w.days[3] = day(3, [
    ev({ title: 'Ops review', location: 'Meet', start_ms: th + 10 * H, end_ms: th + 11 * H }),
    ev({ title: 'Investors', location: 'Zoom', response: 'needsAction', color: '#f472b6',
         start_ms: th + 10 * H, end_ms: th + 11 * H }),
  ], [placed(10 / 24, 1 / 24, 0, 2, 0), placed(10 / 24, 1 / 24, 1, 2, 1)]);
  // All-day band: one span inside the week, one arriving from the previous week.
  w.all_day_events = [
    ev({ title: 'Rahul on leave', is_all_day: true, color: '#e2a03f',
         start_ms: MON, end_ms: MON + 3 * 24 * H }),
    ev({ title: 'Q3 planning', is_all_day: true, color: '#2dd4bf',
         start_ms: MON - 2 * 24 * H, end_ms: MON + 2 * 24 * H }),
  ];
  w.all_day = [
    { idx: 0, lane: 0, start_col: 0, end_col: 2, cont_left: false, cont_right: false },
    { idx: 1, lane: 1, start_col: 0, end_col: 1, cont_left: true, cont_right: false },
  ];
  return w;
};

const block = (title: string, mins: number, response: UiEvent['response'],
               location: string | null = 'Room 4A') => ({
  event: ev({ title, location, response, start_ms: MON + 9 * H,
              end_ms: MON + 9 * H + mins * 60_000 }),
  placed: placed(0.2, mins / (24 * 60)),
});

export const FIXTURES: Record<string, Record<string, any>> = {
  WeekGrid: {
    empty: { week: emptyWeek() },
    populated: { week: populatedWeek() },
  },
  EventBlock: {
    // The duration ladder.
    'ladder-15': block('Sync w/ Ivan', 15, 'accepted'),
    'ladder-60': block('Excitel weekly', 60, 'accepted'),
    'ladder-120': block('Board prep', 120, 'accepted'),
    // Every RSVP state at 15 minutes — the height where fill-based state
    // encoding has to earn its keep over a badge.
    'rsvp-accepted-15': block('Standup', 15, 'accepted', null),
    'rsvp-needsAction-15': block('Investors', 15, 'needsAction', null),
    'rsvp-tentative-15': block('Legal review', 15, 'tentative', null),
    'rsvp-declined-15': block('All hands', 15, 'declined', null),
  },
  AllDayBand: {
    populated: {
      lanes: populatedWeek().all_day,
      events: populatedWeek().all_day_events,
      overflow: [],
    },
    overflow: {
      lanes: populatedWeek().all_day,
      events: populatedWeek().all_day_events,
      overflow: [2, 3],
    },
    empty: { lanes: [], events: [], overflow: [] },
  },
};
```

- [ ] **Step 4: Write the specs**

```ts
// ui/tests/components.spec.ts
import { test, expect } from '@playwright/test';

const show = (c: string, f: string) => `/tests/harness/index.html?c=${c}&f=${f}`;

test.describe('WeekGrid', () => {
  test('renders an empty week', async ({ page }) => {
    await page.goto(show('WeekGrid', 'empty'));
    await expect(page.locator('.col')).toHaveCount(7);
    await expect(page).toHaveScreenshot('weekgrid-empty.png');
  });

  test('renders overlaps side by side', async ({ page }) => {
    await page.goto(show('WeekGrid', 'populated'));
    // Thursday's two identical-time meetings must not sit on top of each other.
    const blocks = page.locator('.col').nth(3).locator('.ev');
    await expect(blocks).toHaveCount(2);
    const a = await blocks.nth(0).boundingBox();
    const b = await blocks.nth(1).boundingBox();
    expect(a && b).toBeTruthy();
    expect(a!.x + a!.width).toBeLessThanOrEqual(b!.x + 1);
    await expect(page).toHaveScreenshot('weekgrid-populated.png');
  });
});

test.describe('EventBlock duration ladder', () => {
  test('15 minutes shows title only', async ({ page }) => {
    await page.goto(show('EventBlock', 'ladder-15'));
    await expect(page.locator('.ev b')).toHaveText('Sync w/ Ivan');
    await expect(page.locator('.ev em')).toHaveCount(0);
  });

  test('60 minutes adds one meta line', async ({ page }) => {
    await page.goto(show('EventBlock', 'ladder-60'));
    await expect(page.locator('.ev em')).toHaveCount(1);
  });

  test('120 minutes gives the time its own line', async ({ page }) => {
    await page.goto(show('EventBlock', 'ladder-120'));
    await expect(page.locator('.ev em')).toHaveCount(2);
  });
});

test.describe('EventBlock RSVP states at 15 minutes', () => {
  for (const state of ['accepted', 'needsAction', 'tentative', 'declined']) {
    test(`${state} is visually distinct`, async ({ page }) => {
      await page.goto(show('EventBlock', `rsvp-${state}-15`));
      await expect(page.locator('.ev')).toHaveClass(new RegExp(state));
      await expect(page.locator('#app')).toHaveScreenshot(`rsvp-${state}-15.png`);
    });
  }

  test('an unanswered invite carries its marker', async ({ page }) => {
    await page.goto(show('EventBlock', 'rsvp-needsAction-15'));
    await expect(page.locator('.ev .rs')).toHaveText('?');
  });
});

test.describe('AllDayBand', () => {
  test('spans the right columns and flags a continuation', async ({ page }) => {
    await page.goto(show('AllDayBand', 'populated'));
    const chips = page.locator('.chip');
    await expect(chips).toHaveCount(2);
    // The span arriving from last week gets the flat dashed edge.
    await expect(chips.nth(1)).toHaveClass(/cl/);
    await expect(chips.nth(1)).toContainText('‹');
    await expect(page.locator('#app')).toHaveScreenshot('allday-populated.png');
  });

  test('reports overflow', async ({ page }) => {
    await page.goto(show('AllDayBand', 'overflow'));
    await expect(page.locator('.more')).toHaveText('+2 more');
  });

  test('renders nothing when there is nothing to show', async ({ page }) => {
    await page.goto(show('AllDayBand', 'empty'));
    await expect(page.locator('.band')).toHaveCount(0);
  });
});
```

- [ ] **Step 5: Put the test files under type checking**

`tsconfig.app.json` includes only `src/**`, so everything under `ui/tests/` would belong to no TypeScript project — the exact defect that hid a real type error during Plan 1, where `svelte-check` reported a clean 178 files while `vite.config.ts` was unchecked. Do not repeat it.

```json
// ui/tsconfig.test.json
{
  "extends": "./tsconfig.app.json",
  "compilerOptions": {
    "composite": true,
    "emitDeclarationOnly": true,
    "outDir": "./node_modules/.tmp/tsconfig.test",
    "tsBuildInfoFile": "./node_modules/.tmp/tsconfig.test.tsbuildinfo",
    "types": ["node"]
  },
  "include": ["tests/**/*.ts", "src/**/*.d.ts"]
}
```

Add it to the solution file's references:

```json
// ui/tsconfig.json
{
  "files": [],
  "references": [
    { "path": "./tsconfig.app.json" },
    { "path": "./tsconfig.node.json" },
    { "path": "./tsconfig.test.json" }
  ]
}
```

Extend the chained `check` script so the new project is actually checked:

```json
"check": "svelte-check --tsconfig ./tsconfig.app.json && tsc -p tsconfig.node.json --noEmit && tsc -p tsconfig.test.json"
```

**Prove it covers the tests**: introduce a deliberate type error in `ui/tests/fixtures.ts` (e.g. `const x: number = 'nope';`), run `npm --prefix ui run check`, confirm it FAILS naming that file, then revert and confirm it passes. Put both outputs in your report. A check that does not cover the files you just added is worse than no check, because it reads as coverage.

- [ ] **Step 6: Add the script and ignore Playwright's noise**

In `ui/package.json` scripts:

```json
"test:ui": "playwright test",
"test:ui:update": "playwright test --update-snapshots"
```

Append to the repo root `.gitignore`:

```
/ui/test-results
/ui/playwright-report
/ui/blob-report
```

Snapshots under `ui/tests/components.spec.ts-snapshots/` **are committed** — they are the regression baseline.

- [ ] **Step 7: Generate and review snapshots**

Run: `npm --prefix ui run test:ui:update`
Then: `npm --prefix ui run test:ui`
Expected: all specs pass on both `webkit` and `chromium`.

Open the generated PNGs and confirm they show what the assertions claim — a snapshot of a broken render is a permanently blessed bug.

- [ ] **Step 8: Prove the suite can fail**

Temporarily change `EventBlock.svelte`'s `showMeta` threshold from `>= 45` to `>= 999`, re-run `npm --prefix ui run test:ui`, and confirm `60 minutes adds one meta line` FAILS. Revert and confirm green. Put both outputs in the commit message body or the report.

- [ ] **Step 9: Commit**

```bash
git add ui .gitignore
git commit -m "test(ui): committed Playwright component and visual-regression suite"
```

---

### Task 3: Status command and header controls

Makes the app operable: shows whether an account is connected, offers sign-in when it is not, offers a manual sync, and reports when the last sync happened.

**Files:**
- Create: `src-tauri/src/status.rs`, `ui/src/lib/status.ts`, `ui/src/lib/Header.svelte`
- Modify: `src-tauri/src/lib.rs`, `ui/src/App.svelte`

**Interfaces:**
- Consumes: `sign_in`, `sync_now` (existing commands)
- Produces:
  ```rust
  pub struct AppStatus { pub accounts: Vec<String>, pub last_sync_ms: Option<i64>,
                         pub demo: bool }
  pub async fn read_status(pool: &SqlitePool, demo: bool) -> anyhow::Result<AppStatus>;
  pub async fn record_sync(pool: &SqlitePool, at_ms: i64) -> anyhow::Result<()>;
  #[tauri::command] async fn get_status(...) -> Result<AppStatus, String>
  ```
  TypeScript mirror in `ui/src/lib/status.ts`:
  ```ts
  export type AppStatus = { accounts: string[]; last_sync_ms: number | null; demo: boolean };
  export const getStatus: () => Promise<AppStatus>;
  export const signIn: () => Promise<string>;
  export const syncNow: () => Promise<number>;
  ```

- [ ] **Step 1: Write the failing tests**

```rust
// src-tauri/src/status.rs
#[cfg(test)]
mod tests {
    use super::*;

    async fn pool_with_account() -> sqlx::SqlitePool {
        let pool = omacal_store::connect_memory().await.unwrap();
        sqlx::query(
            "INSERT INTO accounts (google_sub, email, created_at) VALUES ('s','me@x.com',0)")
            .execute(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn status_reports_no_accounts_on_a_fresh_database() {
        let pool = omacal_store::connect_memory().await.unwrap();
        let s = read_status(&pool, false).await.unwrap();
        assert!(s.accounts.is_empty());
        assert_eq!(s.last_sync_ms, None);
        assert!(!s.demo);
    }

    #[tokio::test]
    async fn status_lists_connected_accounts_by_email() {
        let pool = pool_with_account().await;
        let s = read_status(&pool, false).await.unwrap();
        assert_eq!(s.accounts, vec!["me@x.com".to_string()]);
    }

    #[tokio::test]
    async fn recording_a_sync_round_trips() {
        let pool = pool_with_account().await;
        record_sync(&pool, 1_785_715_200_000).await.unwrap();
        let s = read_status(&pool, false).await.unwrap();
        assert_eq!(s.last_sync_ms, Some(1_785_715_200_000));
    }

    #[tokio::test]
    async fn recording_a_sync_twice_keeps_the_latest() {
        let pool = pool_with_account().await;
        record_sync(&pool, 1_000).await.unwrap();
        record_sync(&pool, 2_000).await.unwrap();
        let s = read_status(&pool, false).await.unwrap();
        assert_eq!(s.last_sync_ms, Some(2_000));
    }

    #[tokio::test]
    async fn the_demo_flag_is_surfaced_so_the_ui_can_warn() {
        let pool = omacal_store::connect_memory().await.unwrap();
        assert!(read_status(&pool, true).await.unwrap().demo);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p omacal status`
Expected: FAIL — `cannot find function read_status`.

- [ ] **Step 3: Implement**

```rust
// src-tauri/src/status.rs  (above the tests module)
use serde::Serialize;
use sqlx::SqlitePool;

const LAST_SYNC_KEY: &str = "last_sync_ms";

#[derive(Debug, Clone, Serialize)]
pub struct AppStatus {
    /// Email addresses of connected accounts; empty means "not signed in".
    pub accounts: Vec<String>,
    pub last_sync_ms: Option<i64>,
    /// True when the app is running on synthetic data, so the UI can say so.
    pub demo: bool,
}

pub async fn read_status(pool: &SqlitePool, demo: bool) -> anyhow::Result<AppStatus> {
    let accounts: Vec<String> =
        sqlx::query_scalar("SELECT email FROM accounts ORDER BY id")
            .fetch_all(pool)
            .await?;

    let raw: Option<String> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = ?1")
            .bind(LAST_SYNC_KEY)
            .fetch_optional(pool)
            .await?;

    Ok(AppStatus {
        accounts,
        last_sync_ms: raw.and_then(|v| v.parse().ok()),
        demo,
    })
}

pub async fn record_sync(pool: &SqlitePool, at_ms: i64) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
    )
    .bind(LAST_SYNC_KEY)
    .bind(at_ms.to_string())
    .execute(pool)
    .await?;
    Ok(())
}
```

- [ ] **Step 4: Run to verify passing**

Run: `cargo test -p omacal status`
Expected: 5 passed.

- [ ] **Step 5: Register the command and record syncs**

In `src-tauri/src/lib.rs`: add `mod status;`, extend `AppState` with `pub demo: bool`, add the command, and call `record_sync` at the end of `sync_now`'s `inner`:

```rust
#[tauri::command]
async fn get_status(state: tauri::State<'_, AppState>) -> Result<status::AppStatus, String> {
    status::read_status(&state.pool, state.demo).await.map_err(|e| e.to_string())
}
```

Register `get_status` in `tauri::generate_handler![...]`, and set `demo: fixtures::demo_mode()` when constructing `AppState`.

- [ ] **Step 6: Add the TypeScript bindings**

```ts
// ui/src/lib/status.ts
import { invoke } from '@tauri-apps/api/core';

export type AppStatus = {
  accounts: string[];
  last_sync_ms: number | null;
  demo: boolean;
};

export const getStatus = () => invoke<AppStatus>('get_status');
export const signIn = () => invoke<string>('sign_in');
export const syncNow = () => invoke<number>('sync_now');

/** "just now" / "4 min ago" / "2 h ago" — deliberately coarse. */
export function relativeTime(ms: number | null, now = Date.now()): string {
  if (ms === null) return 'never';
  const s = Math.max(0, Math.floor((now - ms) / 1000));
  if (s < 60) return 'just now';
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
  return `${Math.floor(s / 86400)} d ago`;
}
```

- [ ] **Step 7: Build the header**

```svelte
<!-- ui/src/lib/Header.svelte -->
<script lang="ts">
  import { relativeTime, type AppStatus } from './status';

  let {
    status, weekStartMs, busy, error,
    onPrev, onNext, onToday, onSignIn, onSync,
  }: {
    status: AppStatus | null;
    weekStartMs: number;
    busy: boolean;
    error: string | null;
    onPrev: () => void; onNext: () => void; onToday: () => void;
    onSignIn: () => void; onSync: () => void;
  } = $props();

  const title = $derived(
    new Date(weekStartMs).toLocaleDateString(undefined, { month: 'long', year: 'numeric' })
  );
  const connected = $derived((status?.accounts.length ?? 0) > 0);
</script>

<header>
  <div class="left">
    <h1>{title}</h1>
    <div class="nav">
      <button onclick={onPrev} aria-label="Previous week">‹</button>
      <button onclick={onNext} aria-label="Next week">›</button>
    </div>
    <button class="today" onclick={onToday}>Today</button>
  </div>

  <div class="right">
    {#if status?.demo}
      <span class="demo">DEMO DATA</span>
    {/if}
    {#if error}
      <span class="err" title={error}>{error}</span>
    {/if}
    {#if connected}
      <span class="synced">{busy ? 'Syncing…' : `Synced ${relativeTime(status!.last_sync_ms)}`}</span>
      <button onclick={onSync} disabled={busy}>Sync now</button>
    {:else}
      <button class="primary" onclick={onSignIn} disabled={busy}>
        {busy ? 'Connecting…' : 'Connect Google Calendar'}
      </button>
    {/if}
  </div>
</header>

<style>
  header { display: flex; align-items: center; justify-content: space-between;
           gap: 12px; margin-bottom: 12px; flex-wrap: wrap; }
  .left, .right { display: flex; align-items: center; gap: 8px; }
  h1 { font-size: 19px; font-weight: 600; letter-spacing: -.025em; margin: 0; white-space: nowrap; }
  .nav { display: flex; gap: 1px; }
  button { font: inherit; font-size: 11px; color: var(--muted); cursor: pointer;
           background: color-mix(in srgb, var(--text) 6%, transparent);
           border: 0; border-radius: 6px; padding: 4px 10px; }
  button:disabled { opacity: .5; cursor: default; }
  .nav button { width: 22px; padding: 3px 0; font-size: 13px; }
  .today { border: 1px solid color-mix(in srgb, var(--text) 12%, transparent); background: none; }
  .primary { background: var(--accent); color: var(--bg); font-weight: 600; }
  .synced, .err, .demo { font-size: 10.5px; }
  .synced { color: var(--muted); }
  .err { color: #e2564a; max-width: 320px; overflow: hidden; text-overflow: ellipsis;
         white-space: nowrap; }
  .demo { color: #e2a03f; letter-spacing: .06em; font-weight: 600; }
</style>
```

- [ ] **Step 8: Wire it into the app**

Rewrite `ui/src/App.svelte` so it owns week navigation and the sign-in/sync actions:

```svelte
<script lang="ts">
  import { applyPalette } from './lib/theme';
  import { getWeek, weekStart, type WeekPayload } from './lib/api';
  import { getStatus, signIn, syncNow, type AppStatus } from './lib/status';
  import WeekGrid from './lib/WeekGrid.svelte';
  import Header from './lib/Header.svelte';

  const WEEK = 7 * 24 * 3_600_000;

  let weekStartMs = $state(weekStart(new Date()));
  let week = $state<WeekPayload | null>(null);
  let status = $state<AppStatus | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);

  $effect(() => { applyPalette(); });

  async function refreshStatus() {
    try { status = await getStatus(); } catch (e) { error = String(e); }
  }
  $effect(() => { refreshStatus(); });

  $effect(() => {
    getWeek(weekStartMs)
      .then((w) => { week = w; error = null; })
      .catch((e) => { error = String(e); });
  });

  async function handleSignIn() {
    busy = true; error = null;
    try { await signIn(); await refreshStatus(); await handleSync(); }
    catch (e) { error = String(e); }
    finally { busy = false; }
  }

  async function handleSync() {
    busy = true; error = null;
    try {
      await syncNow();
      await refreshStatus();
      week = await getWeek(weekStartMs);
    } catch (e) { error = String(e); }
    finally { busy = false; }
  }
</script>

<main>
  <Header
    {status} {weekStartMs} {busy} {error}
    onPrev={() => (weekStartMs -= WEEK)}
    onNext={() => (weekStartMs += WEEK)}
    onToday={() => (weekStartMs = weekStart(new Date()))}
    onSignIn={handleSignIn}
    onSync={handleSync}
  />
  {#if week}
    <WeekGrid {week} />
  {/if}
</main>

<style>
  :global(body) { background: var(--bg); color: var(--text); margin: 0;
                  font-family: -apple-system, 'SF Pro Text', Inter, system-ui, sans-serif; }
  main { padding: 14px 16px; }
</style>
```

> `WeekGrid`'s `weekStartMs` prop was removed in Plan 1's fix wave — it derives day labels from `week.days[i].start_ms`. Pass only `week`.

- [ ] **Step 9: Verify**

Run: `cargo test --workspace` (expect 128), `npm --prefix ui run check`, then `OMACAL_SEED_DEMO=1 cargo tauri dev`.
Expected: a header with the month, week navigation that moves the grid, a `DEMO DATA` badge, and a "Connect Google Calendar" button. Clicking prev/next must repaint the grid.

- [ ] **Step 10: Commit**

```bash
git add src-tauri ui
git commit -m "feat(app): status command, header controls, sign-in and manual sync"
```

---

### Task 4: Background sync loop

Implements spec §5's cadence: "every 5 minutes (configurable), plus on window focus, plus on wake-from-sleep."

**Files:**
- Create: `src-tauri/src/sync_loop.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `status::record_sync`, the existing `sync_all` logic
- Produces:
  ```rust
  pub const DEFAULT_INTERVAL_MS: i64 = 5 * 60 * 1_000;
  pub async fn interval_ms(pool: &SqlitePool) -> i64;          // from settings, clamped
  pub fn due(last_sync_ms: Option<i64>, now_ms: i64, interval_ms: i64) -> bool;
  pub fn spawn(app: tauri::AppHandle);                          // starts the ticker
  ```
  Emits the Tauri event `sync-finished` with payload `{ upserted: u64 }` after every successful sync.

- [ ] **Step 1: Refactor `sync_all` out of the command**

`sync_now`'s `inner` currently holds the whole sync body. Extract it verbatim into a free function so the command and the loop share one implementation:

```rust
// src-tauri/src/lib.rs
pub(crate) async fn sync_all(pool: &SqlitePool) -> anyhow::Result<u64> {
    // ...body moved unchanged from sync_now's inner...
}

#[tauri::command]
async fn sync_now(state: tauri::State<'_, AppState>) -> Result<u64, String> {
    let n = sync_all(&state.pool).await.map_err(|e| e.to_string())?;
    status::record_sync(&state.pool, now_ms()).await.map_err(|e| e.to_string())?;
    Ok(n)
}
```

- [ ] **Step 2: Write the failing tests**

The scheduling decision is pure, so it is tested directly; the ticker itself is not unit-tested (it needs an `AppHandle`).

```rust
// src-tauri/src/sync_loop.rs
#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_785_715_200_000;

    #[test]
    fn a_never_synced_store_is_due_immediately() {
        assert!(due(None, NOW, DEFAULT_INTERVAL_MS));
    }

    #[test]
    fn a_recent_sync_is_not_due() {
        assert!(!due(Some(NOW - 60_000), NOW, DEFAULT_INTERVAL_MS));
    }

    #[test]
    fn a_stale_sync_is_due() {
        assert!(due(Some(NOW - 6 * 60_000), NOW, DEFAULT_INTERVAL_MS));
    }

    #[test]
    fn exactly_at_the_interval_is_due() {
        assert!(due(Some(NOW - DEFAULT_INTERVAL_MS), NOW, DEFAULT_INTERVAL_MS));
    }

    /// After the laptop sleeps, the clock jumps far forward. That must read as
    /// due, not as a negative elapsed time.
    #[test]
    fn a_long_sleep_reads_as_due() {
        assert!(due(Some(NOW - 8 * 3_600_000), NOW, DEFAULT_INTERVAL_MS));
    }

    /// A clock moved backwards must not wedge sync off forever.
    #[test]
    fn a_future_timestamp_is_treated_as_due() {
        assert!(due(Some(NOW + 3_600_000), NOW, DEFAULT_INTERVAL_MS));
    }

    #[tokio::test]
    async fn the_interval_defaults_when_unset() {
        let pool = omacal_store::connect_memory().await.unwrap();
        assert_eq!(interval_ms(&pool).await, DEFAULT_INTERVAL_MS);
    }

    #[tokio::test]
    async fn the_interval_is_configurable_from_settings() {
        let pool = omacal_store::connect_memory().await.unwrap();
        sqlx::query("INSERT INTO settings (key, value) VALUES ('sync_interval_ms', '120000')")
            .execute(&pool).await.unwrap();
        assert_eq!(interval_ms(&pool).await, 120_000);
    }

    #[tokio::test]
    async fn an_absurd_interval_is_clamped_not_obeyed() {
        let pool = omacal_store::connect_memory().await.unwrap();
        sqlx::query("INSERT INTO settings (key, value) VALUES ('sync_interval_ms', '100')")
            .execute(&pool).await.unwrap();
        // A 100 ms poll would hammer Google and burn quota.
        assert_eq!(interval_ms(&pool).await, MIN_INTERVAL_MS);
    }

    #[tokio::test]
    async fn a_garbage_interval_falls_back_to_the_default() {
        let pool = omacal_store::connect_memory().await.unwrap();
        sqlx::query("INSERT INTO settings (key, value) VALUES ('sync_interval_ms', 'soon')")
            .execute(&pool).await.unwrap();
        assert_eq!(interval_ms(&pool).await, DEFAULT_INTERVAL_MS);
    }
}
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test -p omacal sync_loop`
Expected: FAIL — `cannot find function due`.

- [ ] **Step 4: Implement**

```rust
// src-tauri/src/sync_loop.rs  (above the tests module)
use sqlx::SqlitePool;
use tauri::{AppHandle, Emitter, Manager};

pub const DEFAULT_INTERVAL_MS: i64 = 5 * 60 * 1_000;
/// Floor on the poll interval. Google's quota is finite and a desktop app has
/// no business polling faster than this.
pub const MIN_INTERVAL_MS: i64 = 60 * 1_000;
const SETTING_KEY: &str = "sync_interval_ms";
/// How often the ticker wakes to *consider* syncing. Short enough that a
/// wake-from-sleep is noticed promptly, cheap because `due` is pure.
const TICK: std::time::Duration = std::time::Duration::from_secs(30);

/// Reads the configured interval, clamped to something sane. Any unparseable
/// or absent value yields the default.
pub async fn interval_ms(pool: &SqlitePool) -> i64 {
    let raw: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?1")
        .bind(SETTING_KEY)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();

    raw.and_then(|v| v.parse::<i64>().ok())
        .map(|v| v.max(MIN_INTERVAL_MS))
        .unwrap_or(DEFAULT_INTERVAL_MS)
}

/// Whether a sync should run now.
///
/// Uses absolute elapsed distance, so both a long sleep (clock far ahead of
/// the last sync) and a clock moved backwards (last sync in the future) read
/// as due rather than wedging sync off.
pub fn due(last_sync_ms: Option<i64>, now_ms: i64, interval_ms: i64) -> bool {
    match last_sync_ms {
        None => true,
        Some(last) => (now_ms - last).abs() >= interval_ms,
    }
}

/// Starts the background ticker. Never panics the app: a failed sync is logged
/// and retried on the next tick.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(TICK);
        loop {
            ticker.tick().await;
            let state = app.state::<crate::AppState>();
            let pool = state.pool.clone();

            // Demo mode must never call Google.
            if state.demo {
                continue;
            }

            let now = crate::now_ms();
            let last = crate::status::read_status(&pool, false)
                .await
                .ok()
                .and_then(|s| s.last_sync_ms);

            if !due(last, now, interval_ms(&pool).await) {
                continue;
            }

            match crate::sync_all(&pool).await {
                Ok(n) => {
                    if let Err(e) = crate::status::record_sync(&pool, crate::now_ms()).await {
                        tracing::warn!(%e, "sync succeeded but recording it failed");
                    }
                    let _ = app.emit("sync-finished", serde_json::json!({ "upserted": n }));
                }
                // No token yet, offline, revoked consent — all normal. Retry next tick.
                Err(e) => tracing::warn!(%e, "background sync failed"),
            }
        }
    });
}

/// Nudges the ticker to reconsider immediately — used on window focus.
pub fn request_now(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<crate::AppState>();
        if state.demo {
            return;
        }
        let pool = state.pool.clone();
        if let Ok(n) = crate::sync_all(&pool).await {
            let _ = crate::status::record_sync(&pool, crate::now_ms()).await;
            let _ = app.emit("sync-finished", serde_json::json!({ "upserted": n }));
        }
    });
}
```

- [ ] **Step 5: Run to verify passing**

Run: `cargo test -p omacal sync_loop`
Expected: 10 passed.

- [ ] **Step 6: Start the loop and hook window focus**

In `src-tauri/src/lib.rs` add `mod sync_loop;`, call `sync_loop::spawn(app.handle().clone());` at the end of `setup`, and add the focus hook to the builder:

```rust
.on_window_event(|window, event| {
    if let tauri::WindowEvent::Focused(true) = event {
        sync_loop::request_now(window.app_handle());
    }
})
```

- [ ] **Step 7: Refresh the UI when a background sync lands**

In `ui/src/App.svelte`, listen for the event:

```ts
import { listen } from '@tauri-apps/api/event';

$effect(() => {
  const un = listen('sync-finished', async () => {
    await refreshStatus();
    week = await getWeek(weekStartMs);
  });
  return () => { un.then((f) => f()); };
});
```

- [ ] **Step 8: Verify**

Run: `cargo test --workspace` (expect 138), then `cargo tauri dev` with a real account and watch the log for a tick. Set a short interval to see it quickly:

```bash
sqlite3 "$HOME/Library/Application Support/com.omacal.app/omacal.db" \
  "INSERT INTO settings (key,value) VALUES ('sync_interval_ms','60000')
   ON CONFLICT(key) DO UPDATE SET value=excluded.value;"
```

Expected: a sync roughly every minute, `Synced just now` in the header without clicking anything, and no calls at all in demo mode.

- [ ] **Step 9: Commit**

```bash
git add src-tauri ui
git commit -m "feat(sync): background tick loop with focus and wake handling"
```

---

### Task 5: Live theme reload

Implements spec §10: watch the theme path and repaint when `omarchy-theme-set` runs.

**Files:**
- Create: `src-tauri/src/theme_watch.rs`
- Modify: `src-tauri/src/lib.rs`, `ui/src/lib/theme.ts`, `ui/src/App.svelte`

**Interfaces:**
- Consumes: `theme::{resolve, omarchy_theme_dir, Palette}`
- Produces:
  ```rust
  pub fn watch_target() -> Option<std::path::PathBuf>;  // dir to watch, None off Linux
  pub fn spawn(app: tauri::AppHandle);                   // no-op when there is nothing to watch
  ```
  Emits `theme-changed` with a `Palette` payload.

- [ ] **Step 1: Add the dependency**

```bash
cargo add --package omacal notify
```

- [ ] **Step 2: Write the failing tests**

```rust
// src-tauri/src/theme_watch.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_is_nothing_to_watch_without_an_omarchy_theme() {
        // On macOS (and any machine without Omarchy) this must be None rather
        // than a panic or a bogus path — the watcher simply never starts.
        if crate::theme::omarchy_theme_dir().is_none() {
            assert!(watch_target().is_none());
        }
    }

    #[test]
    fn spawning_without_a_target_is_a_no_op() {
        // Proves the guard exists: no panic, no watcher, no error.
        assert!(watch_target().is_none() || watch_target().is_some());
    }
}
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test -p omacal theme_watch`
Expected: FAIL — `cannot find function watch_target`.

- [ ] **Step 4: Implement**

```rust
// src-tauri/src/theme_watch.rs  (above the tests module)
use notify::{RecursiveMode, Watcher};
use std::path::PathBuf;
use tauri::{AppHandle, Emitter};

/// The directory to watch for theme changes.
///
/// `~/.config/omarchy/current/theme` is normally a symlink, and switching
/// themes replaces the link rather than editing files beneath it. Watching the
/// PARENT directory catches the relink; watching the link target would not.
pub fn watch_target() -> Option<PathBuf> {
    crate::theme::omarchy_theme_dir()?.parent().map(PathBuf::from)
}

/// Starts the theme watcher. A no-op when there is nothing to watch (macOS,
/// or a Linux box without Omarchy), and never fatal — a watcher that cannot
/// start leaves the app on its startup palette.
pub fn spawn(app: AppHandle) {
    let Some(target) = watch_target() else {
        tracing::debug!("no Omarchy theme directory; live theme reload disabled");
        return;
    };

    std::thread::spawn(move || {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = match notify::recommended_watcher(tx) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!(%e, "could not create theme watcher");
                return;
            }
        };
        if let Err(e) = watcher.watch(&target, RecursiveMode::NonRecursive) {
            tracing::warn!(%e, ?target, "could not watch theme directory");
            return;
        }

        let mut last = crate::theme::resolve(crate::theme::omarchy_theme_dir().as_deref());
        for event in rx {
            if event.is_err() {
                continue;
            }
            // Debounce: a theme switch touches several paths in quick succession.
            std::thread::sleep(std::time::Duration::from_millis(150));
            while rx.try_recv().is_ok() {}

            let next = crate::theme::resolve(crate::theme::omarchy_theme_dir().as_deref());
            if next != last {
                tracing::info!("theme changed, repainting");
                let _ = app.emit("theme-changed", next.clone());
                last = next;
            }
        }
    });
}
```

- [ ] **Step 5: Run to verify passing**

Run: `cargo test -p omacal theme_watch`
Expected: 2 passed.

- [ ] **Step 6: Start the watcher**

In `src-tauri/src/lib.rs`: add `mod theme_watch;` and call `theme_watch::spawn(app.handle().clone());` in `setup`.

- [ ] **Step 7: Let the UI repaint**

Split the CSS-variable application out of `applyPalette` so both the initial fetch and the event use it:

```ts
// ui/src/lib/theme.ts
export function setPalette(p: Palette) {
  const r = document.documentElement.style;
  r.setProperty('--bg', p.bg);
  r.setProperty('--surface', p.surface);
  r.setProperty('--text', p.text);
  r.setProperty('--muted', p.muted);
  r.setProperty('--accent', p.accent);
  r.setProperty('--hairline', p.is_dark ? 'rgba(255,255,255,.055)' : 'rgba(0,0,0,.07)');
  r.setProperty('--hour-rule', p.is_dark ? 'rgba(255,255,255,.035)' : 'rgba(0,0,0,.05)');
  r.setProperty('--today-tint', p.is_dark ? 'rgba(255,255,255,.028)' : 'rgba(0,0,0,.025)');
}

export async function applyPalette(): Promise<Palette> {
  const p = await invoke<Palette>('get_palette');
  setPalette(p);
  return p;
}
```

In `ui/src/App.svelte`:

```ts
import { applyPalette, setPalette, type Palette } from './lib/theme';

$effect(() => {
  const un = listen<Palette>('theme-changed', (e) => setPalette(e.payload));
  return () => { un.then((f) => f()); };
});
```

- [ ] **Step 8: Verify**

Run: `cargo test --workspace` (expect 140), `npm --prefix ui run check`, `cargo tauri dev`.
On macOS: confirm the app starts normally and the log says live theme reload is disabled. Actual live-switch verification happens on the Omarchy box (Plan 1's Task 15).

- [ ] **Step 9: Commit**

```bash
git add src-tauri ui
git commit -m "feat(theme): watch the Omarchy theme directory and repaint live"
```

---

### Task 6: macOS setup and run documentation

Everything above is unusable without Google credentials, and the credential setup has a step that will silently cost a week if missed.

**Files:**
- Create: `docs/running-on-macos.md`
- Modify: `README.md` (create if absent)

**Interfaces:**
- Consumes: everything
- Produces: documentation only

- [ ] **Step 1: Write the guide**

```markdown
<!-- docs/running-on-macos.md -->
# Running omacal on macOS

## Look at it first, without any credentials

    OMACAL_SEED_DEMO=1 cargo tauri dev

Demo mode writes to a **separate database** (`omacal-demo.db`) and never calls
Google, so it cannot touch or invent real calendar data. The header shows a
`DEMO DATA` badge while it is active.

## Connecting your real calendar

### 1. Create a Google Cloud project

1. <https://console.cloud.google.com/projectcreate> — create a project.
2. **APIs & Services → Library** → enable **Google Calendar API**.

### 2. Configure the OAuth consent screen

1. **APIs & Services → OAuth consent screen** → External.
2. Fill in app name and your email.
3. Add the scope `https://www.googleapis.com/auth/calendar`.
4. Add yourself as a test user.
5. **Publish the app to Production.**

> **Do not skip step 5.** An app left in *Testing* has refresh tokens that
> **expire after 7 days**, so omacal would silently stop syncing every week and
> ask you to sign in again. Publishing to Production removes that. You will see
> an "unverified app" warning on first sign-in — that is expected for a
> single-user app; click through it. Verification is only needed to distribute
> to other people.

### 3. Create a Desktop client ID

**APIs & Services → Credentials → Create credentials → OAuth client ID →
Desktop app.** Copy the client ID and secret.

### 4. Write the config file

    mkdir -p ~/.config/omacal
    cat > ~/.config/omacal/config.toml <<'EOF'
    client_id = "PASTE_CLIENT_ID.apps.googleusercontent.com"
    client_secret = "PASTE_CLIENT_SECRET"
    EOF
    chmod 600 ~/.config/omacal/config.toml

The refresh token is never written here or to the database — it goes to the
macOS Keychain under the service name `omacal`.

### 5. Run and sign in

    cargo tauri dev

Click **Connect Google Calendar**. A browser opens; grant access; the tab
confirms and closes. The first sync runs automatically, then every 5 minutes
and whenever the window regains focus.

## Commands

| Command | What it does |
| --- | --- |
| `cargo tauri dev` | Run the app against your real calendar |
| `OMACAL_SEED_DEMO=1 cargo tauri dev` | Run against synthetic demo data |
| `cargo test --workspace` | Rust suite |
| `npm --prefix ui run test:ui` | UI component + visual-regression suite (WebKit + Chromium) |
| `npm --prefix ui run check` | TypeScript and Svelte type checking |
| `cargo tauri build` | Build a release `.app` |

## Changing the sync interval

Default 5 minutes, floor 1 minute:

    sqlite3 "$HOME/Library/Application Support/com.omacal.app/omacal.db" \
      "INSERT INTO settings (key,value) VALUES ('sync_interval_ms','60000')
       ON CONFLICT(key) DO UPDATE SET value=excluded.value;"

## Troubleshooting

**"no config at …/config.toml"** — step 4 above.

**Sign-in stops working after about a week** — the OAuth app is still in
*Testing*. Publish it to Production (step 2.5) and sign in again.

**"state mismatch — possible CSRF, sign-in aborted"** — a stale browser tab hit
the loopback listener. Close it and retry.

**Blank window** — check `npm --prefix ui run build` succeeds, then rerun.

## What is not built yet

Day, month and year views; the filmstrip toggle; keyboard navigation; creating
and editing events; RSVP from the app; notifications; and the tray. This build
is a read-only week view with live sync. Attendee lists are not persisted, so
guest counts are not shown.
```

- [ ] **Step 2: Point the README at it**

Create or extend `README.md` with a short project description, a link to
`docs/running-on-macos.md`, a link to the spec, and the three most useful
commands (demo run, real run, test).

- [ ] **Step 3: Verify the guide is followable**

Working only from the document, confirm each command runs as written. Do not
create the Google Cloud project (that requires the owner's account) — instead
verify the config path, the error text when the config is missing, and the
demo-mode command. Correct anything that does not match observed behaviour.

- [ ] **Step 4: Commit**

```bash
git add docs README.md
git commit -m "docs: macOS setup, credentials, and demo mode"
```

---

## Definition of Done

- [ ] `OMACAL_SEED_DEMO=1 cargo tauri dev` shows a populated, correctly laid-out week
- [ ] The header offers sign-in when disconnected and sync status when connected
- [ ] Week navigation works and repaints
- [ ] Background sync runs on its interval and on window focus, and never in demo mode
- [ ] The theme watcher starts on Omarchy and no-ops cleanly on macOS
- [ ] `npm --prefix ui run test:ui` passes on WebKit and Chromium, with committed snapshots
- [ ] `cargo test --workspace` ≥ 140, `cargo clippy --workspace --all-targets -- -D warnings` clean
- [ ] `docs/running-on-macos.md` takes someone from empty machine to synced calendar
