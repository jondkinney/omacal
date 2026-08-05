# omacal Plan 1c: Calendar Management, Multi-Account & Polish

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Choose which calendars sync, hide and show them without losing data, connect more than one account, and stop rendering raw conference URLs as location text.

**Architecture:** Today a single `selected` flag gates both *what syncs* and *what displays*, so a hide checkbox built on it would silently stop syncing. This plan splits the two: `sync_enabled` decides whether a calendar is fetched at all, `selected` decides whether it is drawn. A header popover exposes both, and removal persists so `calendarList.list` cannot silently re-import what you removed.

**Tech Stack:** Rust (Tauri v2, sqlx + SQLite, jiff), TypeScript + Svelte 5, Playwright (WebKit + Chromium).

**Spec:** `docs/superpowers/specs/2026-08-05-omacal-design.md`
**Predecessors:** Plan 1 (M0–M1) and Plan 1b (operability), both complete and merged.

## Why this plan exists

Running the merged build against a real account surfaced what synthetic fixtures could not:

- Sign-in imports **every** calendar — primary, secondary, shared, subscribed holidays — with no way to choose. A real account brought in 4 calendars and 665 events at once.
- The `selected` column exists and both the sync loop (`src-tauri/src/lib.rs:321`) and the window query (`crates/omacal-store/src/events.rs:148`) read it, so unchecking a calendar to hide it would also stop it syncing. Re-checking would then show a gap until the next full sync. Hiding must not mean forgetting.
- `sign_in` handles one account; `Header.svelte` only offers Connect when `accounts.length === 0`. The sync loop already iterates all accounts, so this is a UI gap, not an architectural one.
- Real events put conference URLs in `location`, so blocks render `https://us02we…`. The provider name is what is useful at a glance, and `conference_uri` is already stored and never read.
- Plan 1b's final review flagged that `sync_now` and `sign_in` return `sync_all`/`load_config` errors verbatim to the webview. A malformed `config.toml` can therefore render the client secret on screen — the same leak the `sync-failed` event correctly refuses to carry.

## Decisions taken

- **Calendar list lives in a header popover**, not a permanent sidebar. The seven-column grid wants its width, and the spec's stated aim is minimal.
- **Removing a calendar deletes its events and keeps its row**, with `sync_enabled = 0`. The row must persist or the next `calendarList.list` silently re-imports it.

## Deliberately not in this plan

- **Spec §7.2's "declined events are hidden by default, with a toggle to show them"** is still unmet. It is a visibility control like the rest of this plan, but it acts per-event rather than per-calendar and needs a settings surface that does not exist yet. Doing it properly means deciding where app preferences live; bolting a second toggle into the calendar popover would be the wrong home for it. Left for the plan that introduces settings.
- **Per-end timezone display** ("09:00 IST – 13:00 EET" on a flight). `end_tz` has been stored correctly since Plan 1 and is still not rendered.
- **Reordering or recolouring calendars locally.** Google's colours are used as-is; overriding them is user data we would have to own and reconcile.

## Global Constraints

- **Rust edition 2021**; Tauri package `omacal`, lib target `omacal_lib`.
- **`selected` means displayed. `sync_enabled` means fetched.** No code may use one for the other's purpose.
- Time is `i64` epoch milliseconds. `chrono` stays confined to `crates/omacal-core`; `jiff` elsewhere.
- **Never `{:?}`-log, print, or interpolate a `Tokens` value or any token string.** `Tokens` has a redacting `Debug` — do not replace it with a derive.
- **The CSRF check in `sign_in` must not be removed or weakened.** It has no automated coverage.
- **Use `sqlx::query`/`query_as`/`query_scalar` (runtime-checked), never the `query!` macros.**
- **Demo mode must never write to the real database or reach Google.** Its three enforcement points (separate DB, `demo_sync_guard`, `should_sync`/`may_sync`) all stay.
- Svelte 5 runes only — `$props()`, `$state()`, `$derived()`, `$effect()`. No `export let`, no `$:`.
- **No live network calls in tests.**
- `cargo test --workspace` starts at **163** and must never regress. `npm --prefix ui run test:ui` starts at **66**.

---

## File Structure

| Path | Responsibility |
| --- | --- |
| `crates/omacal-store/migrations/0002_sync_enabled.sql` | Adds `sync_enabled`, backfilled from `selected` |
| `crates/omacal-store/src/calendars.rs` | Calendar list/toggle/remove queries |
| `src-tauri/src/calendars.rs` | Calendar Tauri commands |
| `src-tauri/src/errors.rs` | Sanitising errors that cross into the webview |
| `ui/src/lib/calendars.ts` | Calendar bindings and types |
| `ui/src/lib/CalendarPopover.svelte` | The picker |
| `ui/src/lib/location.ts` | Conference-URL → provider label |

---

### Task 1: Split `sync_enabled` from `selected`

Everything else rests on this. Until the two flags are distinct, a hide checkbox would silently stop syncing.

**Files:**
- Create: `crates/omacal-store/migrations/0002_sync_enabled.sql`
- Modify: `crates/omacal-store/src/events.rs`, `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: the existing `calendars` table
- Produces: a `sync_enabled INTEGER NOT NULL DEFAULT 1` column. `events_in_window` continues to filter on `c.selected = 1`; the sync loop's calendar query switches to `sync_enabled = 1`.

- [ ] **Step 1: Write the migration**

```sql
-- crates/omacal-store/migrations/0002_sync_enabled.sql

-- `selected` used to gate both fetching and drawing. Splitting them: this
-- column decides whether a calendar is synced at all, `selected` decides
-- whether it is drawn. Existing rows keep current behaviour — anything being
-- displayed was also being synced.
ALTER TABLE calendars ADD COLUMN sync_enabled INTEGER NOT NULL DEFAULT 1;
UPDATE calendars SET sync_enabled = selected;
```

- [ ] **Step 2: Write the failing tests**

```rust
// crates/omacal-store/src/events.rs — add to the existing tests module

#[tokio::test]
async fn hiding_a_calendar_does_not_stop_it_syncing() {
    let pool = connect_memory().await.unwrap();
    let cal = seed(&pool).await;
    upsert_event(&pool, &ev(cal, "a", 1000, 2000)).await.unwrap();

    // Hidden, but still synced: the whole point of the split.
    sqlx::query("UPDATE calendars SET selected = 0").execute(&pool).await.unwrap();

    assert!(events_in_window(&pool, 0, 5000).await.unwrap().is_empty(),
            "a hidden calendar must not render");

    let syncing: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM calendars WHERE sync_enabled = 1")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(syncing, 1, "hiding must not disable syncing");
}

#[tokio::test]
async fn the_migration_backfills_sync_enabled_from_selected() {
    let pool = connect_memory().await.unwrap();
    let cal = seed(&pool).await;
    let _ = cal;
    let (selected, sync_enabled): (i64, i64) = sqlx::query_as(
        "SELECT selected, sync_enabled FROM calendars LIMIT 1")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(selected, 1);
    assert_eq!(sync_enabled, 1, "a fresh calendar syncs by default");
}
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test -p omacal-store hiding_a_calendar`
Expected: FAIL — `no such column: sync_enabled`.

- [ ] **Step 4: Point the sync loop at the new flag**

In `src-tauri/src/lib.rs`, the calendar query inside `sync_all` currently reads:

```rust
"SELECT id, google_id FROM calendars WHERE account_id = ?1 AND selected = 1",
```

Change it to:

```rust
// `sync_enabled`, not `selected`: hiding a calendar in the UI must not stop it
// syncing, or re-showing it would reveal a gap until the next full sync.
"SELECT id, google_id FROM calendars WHERE account_id = ?1 AND sync_enabled = 1",
```

Leave `events_in_window`'s `c.selected = 1` exactly as it is — that one is about drawing.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p omacal-store` then `cargo test --workspace`
Expected: both green; state the new workspace total.

- [ ] **Step 6: Commit**

```bash
git add crates/omacal-store src-tauri
git commit -m "feat(store): separate sync_enabled from selected"
```

---

### Task 2: Calendar queries

**Files:**
- Create: `crates/omacal-store/src/calendars.rs`
- Modify: `crates/omacal-store/src/lib.rs`

**Interfaces:**
- Consumes: `omacal_store::connect_memory`
- Produces:
  ```rust
  pub struct CalendarRow {
      pub id: i64, pub account_id: i64, pub account_email: String,
      pub summary: String, pub color_hex: Option<String>,
      pub selected: bool, pub sync_enabled: bool, pub is_primary: bool,
  }
  pub async fn list_calendars(pool: &SqlitePool) -> anyhow::Result<Vec<CalendarRow>>;
  pub async fn set_selected(pool: &SqlitePool, id: i64, on: bool) -> anyhow::Result<()>;
  pub async fn set_sync_enabled(pool: &SqlitePool, id: i64, on: bool) -> anyhow::Result<u64>;
  ```
  `set_sync_enabled(.., false)` also deletes that calendar's events and returns how many were removed. Disabling sync and keeping stale events would leave the store growing with data that never updates.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/omacal-store/src/calendars.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{connect_memory, upsert_event, StoredEvent};

    async fn seed(pool: &SqlitePool) {
        sqlx::query("INSERT INTO accounts (google_sub, email, created_at)
                     VALUES ('s','me@x.com',0)").execute(pool).await.unwrap();
        for (gid, name, primary) in [("primary", "Work", 1), ("hols", "Holidays", 0)] {
            sqlx::query(
                "INSERT INTO calendars
                     (account_id, google_id, summary, color_hex, timezone, access_role, is_primary)
                 VALUES (1, ?1, ?2, '#5b8def', 'UTC', 'owner', ?3)")
                .bind(gid).bind(name).bind(primary)
                .execute(pool).await.unwrap();
        }
    }

    fn ev(cal: i64, gid: &str) -> StoredEvent {
        StoredEvent {
            id: 0, calendar_id: cal, google_id: gid.into(), summary: Some("x".into()),
            location: None, start_utc: 1000, end_utc: 2000,
            start_tz: "UTC".into(), end_tz: "UTC".into(), is_all_day: false,
            recurrence: None, recurring_event_id: None, original_start_utc: None,
            status: "confirmed".into(), self_response: None, conference_uri: None,
            color_hex: None,
        }
    }

    #[tokio::test]
    async fn listing_returns_every_calendar_with_its_account() {
        let pool = connect_memory().await.unwrap();
        seed(&pool).await;
        let cals = list_calendars(&pool).await.unwrap();
        assert_eq!(cals.len(), 2);
        assert!(cals.iter().all(|c| c.account_email == "me@x.com"));
        assert!(cals.iter().any(|c| c.is_primary));
    }

    #[tokio::test]
    async fn listing_puts_the_primary_calendar_first() {
        let pool = connect_memory().await.unwrap();
        seed(&pool).await;
        let cals = list_calendars(&pool).await.unwrap();
        assert!(cals[0].is_primary, "the primary calendar should lead the list");
    }

    #[tokio::test]
    async fn hiding_a_calendar_keeps_its_events_and_its_sync() {
        let pool = connect_memory().await.unwrap();
        seed(&pool).await;
        upsert_event(&pool, &ev(1, "a")).await.unwrap();

        set_selected(&pool, 1, false).await.unwrap();

        let row = list_calendars(&pool).await.unwrap();
        let c = row.iter().find(|c| c.id == 1).unwrap();
        assert!(!c.selected, "hidden");
        assert!(c.sync_enabled, "still syncing — hiding is not removing");

        let kept: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE calendar_id = 1")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(kept, 1, "hiding must not discard data");
    }

    #[tokio::test]
    async fn removing_a_calendar_deletes_its_events_but_keeps_the_row() {
        let pool = connect_memory().await.unwrap();
        seed(&pool).await;
        upsert_event(&pool, &ev(1, "a")).await.unwrap();
        upsert_event(&pool, &ev(1, "b")).await.unwrap();
        upsert_event(&pool, &ev(2, "c")).await.unwrap();

        let removed = set_sync_enabled(&pool, 1, false).await.unwrap();
        assert_eq!(removed, 2);

        let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE calendar_id = 1")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(left, 0);

        let other: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE calendar_id = 2")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(other, 1, "removing one calendar must not touch another");

        // The row survives so the calendar can be re-enabled, and so the next
        // calendarList.list cannot silently re-import what was removed.
        let still_listed = list_calendars(&pool).await.unwrap();
        assert!(still_listed.iter().any(|c| c.id == 1 && !c.sync_enabled));
    }

    #[tokio::test]
    async fn re_enabling_a_calendar_leaves_it_ready_to_refetch() {
        let pool = connect_memory().await.unwrap();
        seed(&pool).await;
        upsert_event(&pool, &ev(1, "a")).await.unwrap();
        // Plant a sync cursor, or the assertion below passes whether or not the
        // code deletes anything — `seed` creates no sync_state row of its own.
        sqlx::query(
            "INSERT INTO sync_state (calendar_id, sync_token, window_start, window_end)
             VALUES (1, 'stale-token', 0, 0)")
            .execute(&pool).await.unwrap();

        set_sync_enabled(&pool, 1, false).await.unwrap();
        set_sync_enabled(&pool, 1, true).await.unwrap();

        let c = list_calendars(&pool).await.unwrap();
        let c = c.iter().find(|c| c.id == 1).unwrap();
        assert!(c.sync_enabled);
        // The cursor went with the events. Keeping it would make the next sync
        // ask Google for a diff against events that are no longer here, and the
        // calendar would come back empty until the token went stale on its own.
        let tok: Option<String> = sqlx::query_scalar(
            "SELECT sync_token FROM sync_state WHERE calendar_id = 1")
            .fetch_optional(&pool).await.unwrap().flatten();
        assert!(tok.is_none(), "a re-enabled calendar must resync from scratch");
    }

    #[tokio::test]
    async fn toggling_an_unknown_calendar_is_not_an_error() {
        let pool = connect_memory().await.unwrap();
        seed(&pool).await;
        // The popover can race a sync that removed a calendar; a no-op beats a
        // failure the user cannot act on.
        assert!(set_selected(&pool, 999, false).await.is_ok());
        assert_eq!(set_sync_enabled(&pool, 999, false).await.unwrap(), 0);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p omacal-store calendars`
Expected: FAIL — `cannot find function list_calendars`.

- [ ] **Step 3: Implement**

```rust
// crates/omacal-store/src/calendars.rs  (above the tests module)
use serde::Serialize;
use sqlx::{Row, SqlitePool};

#[derive(Debug, Clone, Serialize)]
pub struct CalendarRow {
    pub id: i64,
    pub account_id: i64,
    pub account_email: String,
    pub summary: String,
    pub color_hex: Option<String>,
    /// Drawn in the grid.
    pub selected: bool,
    /// Fetched from Google at all.
    pub sync_enabled: bool,
    pub is_primary: bool,
}

/// Every calendar across every account, primary first, then alphabetical —
/// stable ordering so the popover does not reshuffle between renders.
pub async fn list_calendars(pool: &SqlitePool) -> anyhow::Result<Vec<CalendarRow>> {
    let rows = sqlx::query(
        "SELECT c.id, c.account_id, a.email AS account_email, c.summary,
                c.color_hex, c.selected, c.sync_enabled, c.is_primary
         FROM calendars c
         JOIN accounts a ON a.id = c.account_id
         ORDER BY a.email, c.is_primary DESC, c.summary COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .iter()
        .map(|r| CalendarRow {
            id: r.get("id"),
            account_id: r.get("account_id"),
            account_email: r.get("account_email"),
            summary: r.get("summary"),
            color_hex: r.get("color_hex"),
            selected: r.get::<i64, _>("selected") != 0,
            sync_enabled: r.get::<i64, _>("sync_enabled") != 0,
            is_primary: r.get::<i64, _>("is_primary") != 0,
        })
        .collect())
}

/// Show or hide a calendar. Pure display — no data is fetched or discarded.
pub async fn set_selected(pool: &SqlitePool, id: i64, on: bool) -> anyhow::Result<()> {
    sqlx::query("UPDATE calendars SET selected = ?2 WHERE id = ?1")
        .bind(id)
        .bind(on as i64)
        .execute(pool)
        .await?;
    Ok(())
}

/// Add or remove a calendar from syncing.
///
/// Turning it off deletes its events and its sync cursor: keeping stale rows
/// that never update again would grow the store for no benefit, and a stale
/// `syncToken` would make the next re-enable fetch an incremental diff against
/// events that are no longer there. Returns the number of events removed.
pub async fn set_sync_enabled(pool: &SqlitePool, id: i64, on: bool) -> anyhow::Result<u64> {
    let mut tx = pool.begin().await?;

    sqlx::query("UPDATE calendars SET sync_enabled = ?2 WHERE id = ?1")
        .bind(id)
        .bind(on as i64)
        .execute(&mut *tx)
        .await?;

    let removed = if on {
        0
    } else {
        let n = sqlx::query("DELETE FROM events WHERE calendar_id = ?1")
            .bind(id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        sqlx::query("DELETE FROM sync_state WHERE calendar_id = ?1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        n
    };

    tx.commit().await?;
    Ok(removed)
}
```

- [ ] **Step 4: Wire into the crate root**

```rust
// crates/omacal-store/src/lib.rs — add
pub mod calendars;
pub use calendars::{list_calendars, set_selected, set_sync_enabled, CalendarRow};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p omacal-store`
Expected: 6 new tests pass; state the crate total.

- [ ] **Step 6: Commit**

```bash
git add crates/omacal-store
git commit -m "feat(store): calendar list, show/hide, and remove"
```

---

### Task 3: Calendar commands and sanitised errors

Two things at once because they touch the same file and the same risk: what crosses from Rust into the webview.

**Files:**
- Create: `src-tauri/src/calendars.rs`, `src-tauri/src/errors.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `omacal_store::{list_calendars, set_selected, set_sync_enabled, CalendarRow}`
- Produces:
  ```rust
  // errors.rs
  pub fn user_facing(err: &anyhow::Error) -> String;
  // calendars.rs — Tauri commands
  #[tauri::command] async fn get_calendars(..) -> Result<Vec<CalendarRow>, String>
  #[tauri::command] async fn set_calendar_selected(.., id: i64, on: bool) -> Result<(), String>
  #[tauri::command] async fn set_calendar_sync(.., id: i64, on: bool) -> Result<u64, String>
  ```

- [ ] **Step 1: Write the failing tests for error sanitising**

```rust
// src-tauri/src/errors.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_toml_parse_error_never_reaches_the_user_verbatim() {
        // toml's Display quotes the offending source line, which for this file
        // is the client secret. Plan 1b established this for the sync-failed
        // event; the command return path had the same hole.
        let src = "client_id = \"x\"\nclient_secret = GOCSPX-pretend-secret\n";
        let err: anyhow::Error = toml::from_str::<toml::Value>(src).unwrap_err().into();
        let shown = user_facing(&err);
        assert!(!shown.contains("GOCSPX"), "secret leaked to the UI: {shown}");
    }

    #[test]
    fn a_url_bearing_error_is_not_shown_verbatim() {
        // reqwest's Display carries the whole request URL, sync tokens included.
        let err = anyhow::anyhow!(
            "error sending request for url (https://x/events?syncToken=CPjO_SECRET)"
        );
        let shown = user_facing(&err);
        assert!(!shown.contains("syncToken"), "sync token leaked: {shown}");
        assert!(!shown.contains("CPjO_SECRET"));
    }

    #[test]
    fn a_safe_message_is_passed_through_so_the_user_can_act() {
        // The missing-config message names the file to create — losing it would
        // make the most common first-run failure unactionable.
        let err = anyhow::anyhow!(
            "no config at /Users/x/.config/omacal/config.toml: No such file or directory (os error 2). Create it with client_id and client_secret."
        );
        let shown = user_facing(&err);
        assert!(shown.contains("config.toml"));
        assert!(shown.contains("client_id"));
    }

    #[test]
    fn an_unrecognised_error_falls_back_to_something_generic() {
        let err = anyhow::anyhow!("Bearer ya29.a0AfB_pretend_access_token failed");
        let shown = user_facing(&err);
        assert!(!shown.contains("ya29"), "access token leaked: {shown}");
        assert!(!shown.is_empty());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p omacal errors`
Expected: FAIL — `cannot find function user_facing`.

- [ ] **Step 3: Implement**

```rust
// src-tauri/src/errors.rs  (above the tests module)

/// Patterns that mean an error string is carrying something secret.
///
/// Deny-list rather than allow-list is the wrong shape in general, but the safe
/// messages here are few and known, so the fallback below is what actually
/// protects us: anything not explicitly recognised is replaced wholesale.
const SECRET_MARKERS: &[&str] = &[
    "GOCSPX",       // Google client secret prefix
    "syncToken",    // appears in a request URL
    "client_secret",
    "Bearer",
    "ya29.",        // Google access token prefix
    "1//",          // Google refresh token prefix
];

/// The generic replacement. Deliberately says where to look rather than
/// pretending nothing happened.
const OPAQUE: &str = "Sync failed. See the application log for details.";

/// Renders an error for display in the webview.
///
/// Errors reach the UI through two channels — the `sync-failed` event and a
/// command's `Err` return — and both end up in the same header element. The
/// event channel already refuses to carry error detail; this is the other one.
pub fn user_facing(err: &anyhow::Error) -> String {
    let text = err.to_string();

    if SECRET_MARKERS.iter().any(|m| text.contains(m)) {
        return OPAQUE.to_string();
    }

    // The missing-config message is the most likely first-run failure and names
    // the file to create, so it is worth passing through intact.
    if text.contains("config.toml") {
        return text;
    }

    // Anything else: keep it only if it is short and has no URL in it. A long
    // error is usually a wrapped chain carrying more than the user needs.
    if text.len() <= 160 && !text.contains("://") {
        return text;
    }

    OPAQUE.to_string()
}
```

- [ ] **Step 4: Run to verify passing**

Run: `cargo test -p omacal errors`
Expected: 4 passed.

- [ ] **Step 5: Route the command returns through it**

In `src-tauri/src/lib.rs`, `sync_now` and `sign_in` currently do `.map_err(|e| e.to_string())`. Change both to use the sanitiser:

```rust
// sync_now
let n = sync_all(&state).await.map_err(|e| errors::user_facing(&e))?;
```

```rust
// sign_in_impl's tail — where `inner(pool).await` is mapped
inner(pool).await.map_err(|e| errors::user_facing(&e))
```

Add `mod errors;` to `lib.rs`. Do not change `demo_sync_guard`'s message — it is already a plain sentence and contains nothing secret.

- [ ] **Step 6: Write the calendar commands**

```rust
// src-tauri/src/calendars.rs
use crate::AppState;
use omacal_store::CalendarRow;

#[tauri::command]
pub async fn get_calendars(state: tauri::State<'_, AppState>) -> Result<Vec<CalendarRow>, String> {
    omacal_store::list_calendars(&state.pool)
        .await
        .map_err(|e| crate::errors::user_facing(&e))
}

#[tauri::command]
pub async fn set_calendar_selected(
    state: tauri::State<'_, AppState>,
    id: i64,
    on: bool,
) -> Result<(), String> {
    omacal_store::set_selected(&state.pool, id, on)
        .await
        .map_err(|e| crate::errors::user_facing(&e))
}

/// Returns how many events were removed, so the UI can say what happened.
#[tauri::command]
pub async fn set_calendar_sync(
    state: tauri::State<'_, AppState>,
    id: i64,
    on: bool,
) -> Result<u64, String> {
    omacal_store::set_sync_enabled(&state.pool, id, on)
        .await
        .map_err(|e| crate::errors::user_facing(&e))
}
```

Add `mod calendars;` to `lib.rs` and register all three in `tauri::generate_handler![...]`.

- [ ] **Step 7: Verify**

Run: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo check --workspace`.
State the new workspace total.

- [ ] **Step 8: Commit**

```bash
git add src-tauri
git commit -m "feat(app): calendar commands and sanitised error returns"
```

---

### Task 4: Conference URLs stop rendering as location text

**Files:**
- Create: `ui/src/lib/location.ts`
- Modify: `ui/src/lib/EventBlock.svelte`, `ui/tests/components.spec.ts`

**Interfaces:**
- Consumes: nothing
- Produces: `export function locationLabel(raw: string | null): string`

- [ ] **Step 1: Write the failing tests**

```ts
// ui/tests/location.spec.ts
import { test, expect } from '@playwright/test';
import { locationLabel } from '../src/lib/location';

test.describe('locationLabel', () => {
  test('a plain place is left alone', () => {
    expect(locationLabel('TAO Office, board room')).toBe('TAO Office, board room');
    expect(locationLabel('Room 4A')).toBe('Room 4A');
  });

  test('nothing in, nothing out', () => {
    expect(locationLabel(null)).toBe('');
    expect(locationLabel('   ')).toBe('');
  });

  // Real events put the joining link in `location`, which rendered as
  // `https://us02we…` — the truncation of a URL tells you nothing.
  test('known providers become their name', () => {
    expect(locationLabel('https://us02web.zoom.us/j/123456?pwd=x')).toBe('Zoom');
    expect(locationLabel('https://meet.google.com/abc-defg-hij')).toBe('Google Meet');
    expect(locationLabel('https://teams.microsoft.com/l/meetup-join/x')).toBe('Teams');
  });

  test('a labelled link keeps its label', () => {
    expect(locationLabel('Zoom: https://us02web.zoom.us/j/1')).toBe('Zoom');
  });

  test('an unknown link becomes its host, not a truncated URL', () => {
    expect(locationLabel('https://whereby.com/omacal-standup')).toBe('whereby.com');
  });

  test('a place with a link keeps the place', () => {
    // Google often writes "Room 4A, https://meet.google.com/x". The room is
    // what you act on when you are walking somewhere.
    expect(locationLabel('Room 4A, https://meet.google.com/abc')).toBe('Room 4A');
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `npm --prefix ui run test:ui -- --project=chromium -g locationLabel`
Expected: FAIL — cannot resolve `../src/lib/location`.

- [ ] **Step 3: Implement**

```ts
// ui/src/lib/location.ts

// Matched against the URL's host. Order does not matter; hosts are distinct.
const PROVIDERS: Array<[RegExp, string]> = [
  [/(^|\.)zoom\.us$/i, 'Zoom'],
  [/(^|\.)meet\.google\.com$/i, 'Google Meet'],
  [/(^|\.)teams\.microsoft\.com$/i, 'Teams'],
  [/(^|\.)teams\.live\.com$/i, 'Teams'],
  [/(^|\.)webex\.com$/i, 'Webex'],
  [/(^|\.)meet\.jit\.si$/i, 'Jitsi'],
];

const URL_RE = /https?:\/\/[^\s,;]+/i;

/**
 * What to print in an event block's meta line.
 *
 * Google puts the joining link in `location` as well as in conference data, so
 * a naive render shows `https://us02we…` — a truncated URL, which tells you
 * nothing at a glance. A real place always wins over a link; a link alone
 * becomes its provider's name, or failing that its host.
 */
export function locationLabel(raw: string | null): string {
  const text = (raw ?? '').trim();
  if (!text) return '';

  const match = text.match(URL_RE);
  if (!match) return text;

  // A place written alongside the link is the useful half. Strip the URL and
  // any label that was only introducing it.
  const withoutUrl = text
    .replace(URL_RE, '')
    .replace(/[\s,;–—-]*$/, '')
    .replace(/^[\s,;–—-]*/, '')
    .replace(/[:\s]*$/, '')
    .trim();

  let host = '';
  try {
    host = new URL(match[0]).hostname;
  } catch {
    return withoutUrl || text;
  }

  const provider = PROVIDERS.find(([re]) => re.test(host))?.[1];

  // "Zoom: https://…" — the leading word is just naming the link, not a place.
  if (withoutUrl && provider && withoutUrl.toLowerCase() === provider.toLowerCase()) {
    return provider;
  }
  if (withoutUrl) return withoutUrl;

  return provider ?? host;
}
```

- [ ] **Step 4: Use it in the block**

In `ui/src/lib/EventBlock.svelte`, replace the meta derivation:

```svelte
import { locationLabel } from './location';
...
const meta = $derived(locationLabel(event.location));
```

- [ ] **Step 5: Run to verify passing**

Run: `npm --prefix ui run test:ui -- -g locationLabel`
Expected: 6 tests pass on both engines.

- [ ] **Step 6: Commit**

```bash
git add ui
git commit -m "feat(ui): show the provider name instead of a raw conference URL"
```

---

### Task 5: The calendar popover

**Files:**
- Create: `ui/src/lib/calendars.ts`, `ui/src/lib/CalendarPopover.svelte`
- Modify: `ui/src/lib/Header.svelte`, `ui/src/App.svelte`

**Interfaces:**
- Consumes: `get_calendars`, `set_calendar_selected`, `set_calendar_sync`
- Produces:
  ```ts
  export type Calendar = {
    id: number; account_id: number; account_email: string; summary: string;
    color_hex: string | null; selected: boolean; sync_enabled: boolean; is_primary: boolean;
  };
  export const getCalendars: () => Promise<Calendar[]>;
  export const setCalendarSelected: (id: number, on: boolean) => Promise<void>;
  export const setCalendarSync: (id: number, on: boolean) => Promise<number>;
  ```

- [ ] **Step 1: Add the bindings**

```ts
// ui/src/lib/calendars.ts
import { invoke } from '@tauri-apps/api/core';

export type Calendar = {
  id: number;
  account_id: number;
  account_email: string;
  summary: string;
  color_hex: string | null;
  /** Drawn in the grid. */
  selected: boolean;
  /** Fetched from Google at all. */
  sync_enabled: boolean;
  is_primary: boolean;
};

export const getCalendars = () => invoke<Calendar[]>('get_calendars');
export const setCalendarSelected = (id: number, on: boolean) =>
  invoke<void>('set_calendar_selected', { id, on });
export const setCalendarSync = (id: number, on: boolean) =>
  invoke<number>('set_calendar_sync', { id, on });

/** Calendars grouped by account, preserving the order the backend returned. */
export function byAccount(cals: Calendar[]): Array<[string, Calendar[]]> {
  const groups = new Map<string, Calendar[]>();
  for (const c of cals) {
    const g = groups.get(c.account_email) ?? [];
    g.push(c);
    groups.set(c.account_email, g);
  }
  return [...groups.entries()];
}
```

- [ ] **Step 2: Build the popover**

```svelte
<!-- ui/src/lib/CalendarPopover.svelte -->
<script lang="ts">
  import { byAccount, setCalendarSelected, setCalendarSync, type Calendar } from './calendars';

  let { calendars, onchange }: { calendars: Calendar[]; onchange: () => void } = $props();

  let open = $state(false);
  let busy = $state<number | null>(null);

  const shown = $derived(calendars.filter((c) => c.sync_enabled && c.selected).length);
  const groups = $derived(byAccount(calendars));

  async function toggleShown(c: Calendar) {
    busy = c.id;
    try { await setCalendarSelected(c.id, !c.selected); onchange(); }
    finally { busy = null; }
  }

  async function toggleSync(c: Calendar) {
    busy = c.id;
    try { await setCalendarSync(c.id, !c.sync_enabled); onchange(); }
    finally { busy = null; }
  }
</script>

<div class="wrap">
  <button class="trigger" onclick={() => (open = !open)} aria-expanded={open}>
    Calendars <span class="count">{shown}</span>
  </button>

  {#if open}
    <!-- Click-away. Deliberately a sibling rather than a document listener:
         no global state to leak if the component unmounts while open. -->
    <button class="scrim" aria-label="Close" onclick={() => (open = false)}></button>

    <div class="panel" role="group" aria-label="Calendars">
      {#each groups as [email, cals]}
        <div class="acct">{email}</div>
        {#each cals as c}
          <div class="row" class:off={!c.sync_enabled}>
            <label>
              <input
                type="checkbox"
                checked={c.selected}
                disabled={!c.sync_enabled || busy === c.id}
                onchange={() => toggleShown(c)}
              />
              <s class="dot" style="background:{c.color_hex ?? 'var(--accent)'}"></s>
              <span class="name" title={c.summary}>{c.summary}</span>
            </label>
            <button
              class="sync"
              disabled={busy === c.id}
              title={c.sync_enabled
                ? 'Stop syncing and delete this calendar’s local events'
                : 'Sync this calendar again'}
              onclick={() => toggleSync(c)}
            >{c.sync_enabled ? 'Remove' : 'Add'}</button>
          </div>
        {/each}
      {/each}
      <p class="hint">
        Unticking hides a calendar. Removing stops syncing it and deletes its
        local events; you can add it back.
      </p>
    </div>
  {/if}
</div>

<style>
  .wrap { position: relative; }
  .trigger { font: inherit; font-size: 11px; color: var(--muted); cursor: pointer;
             background: color-mix(in srgb, var(--text) 6%, transparent);
             border: 0; border-radius: 6px; padding: 4px 10px; }
  .count { opacity: .7; margin-left: 4px; }

  .scrim { position: fixed; inset: 0; background: none; border: 0; cursor: default; z-index: 40; }

  .panel { position: absolute; right: 0; top: calc(100% + 6px); z-index: 41;
           min-width: 260px; max-height: 60vh; overflow-y: auto;
           background: var(--surface); border: 1px solid var(--hairline);
           border-radius: 8px; padding: 8px; box-shadow: 0 8px 28px rgba(0,0,0,.45); }

  .acct { font-size: 9.5px; color: var(--muted); letter-spacing: .05em;
          padding: 6px 6px 3px; }
  .row { display: flex; align-items: center; justify-content: space-between; gap: 8px;
         padding: 3px 6px; border-radius: 5px; }
  .row:hover { background: color-mix(in srgb, var(--text) 5%, transparent); }
  .row.off .name { opacity: .45; }

  label { display: flex; align-items: center; gap: 7px; font-size: 11.5px;
          cursor: pointer; min-width: 0; }
  .dot { width: 8px; height: 8px; border-radius: 2.5px; flex: none;
         text-decoration: none; display: block; }
  .name { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }

  .sync { font: inherit; font-size: 10px; color: var(--muted); cursor: pointer;
          background: none; border: 1px solid var(--hairline); border-radius: 5px;
          padding: 2px 7px; flex: none; }

  .hint { font-size: 9.5px; color: var(--muted); opacity: .8; line-height: 1.45;
          margin: 8px 6px 2px; }
</style>
```

- [ ] **Step 3: Put it in the header**

`Header.svelte` gains `calendars` and `oncalendarchange` props and renders `<CalendarPopover {calendars} onchange={oncalendarchange} />` in its right-hand group, before the sync status. Show it only when `calendars.length > 0`, so a disconnected user sees just the Connect button.

- [ ] **Step 4: Load and refresh in the app**

`App.svelte` holds `let calendars = $state<Calendar[]>([])`, loads them in the same effect that loads status, and on `oncalendarchange` reloads both the calendar list and the current week — a hide takes effect immediately because `events_in_window` filters on `selected`.

- [ ] **Step 5: Write the specs**

Extend `ui/tests/fixtures.ts` with calendar fixtures and add to `ui/tests/components.spec.ts`:

```ts
test.describe('CalendarPopover', () => {
  const show = (f: string) => `/tests/harness/index.html?c=CalendarPopover&f=${f}`;

  test('opens and groups by account', async ({ page }) => {
    await page.goto(show('two-accounts'));
    await page.getByRole('button', { name: /Calendars/ }).click();
    await expect(page.locator('.acct')).toHaveCount(2);
  });

  test('counts only calendars that are both synced and shown', async ({ page }) => {
    await page.goto(show('mixed'));
    // 3 calendars: one hidden, one removed, one visible.
    await expect(page.locator('.trigger .count')).toHaveText('1');
  });

  test('a removed calendar cannot be ticked', async ({ page }) => {
    await page.goto(show('mixed'));
    await page.getByRole('button', { name: /Calendars/ }).click();
    const off = page.locator('.row.off').first();
    await expect(off.locator('input[type=checkbox]')).toBeDisabled();
    await expect(off.locator('.sync')).toHaveText('Add');
  });

  test('clicking away closes it', async ({ page }) => {
    await page.goto(show('two-accounts'));
    await page.getByRole('button', { name: /Calendars/ }).click();
    await expect(page.locator('.panel')).toBeVisible();
    await page.locator('.scrim').click();
    await expect(page.locator('.panel')).toHaveCount(0);
  });
});
```

Add `CalendarPopover` to `ui/tests/harness/mount.ts`'s component map, stubbing the three invokes so a toggle does not need a backend.

- [ ] **Step 6: Verify**

Run: `npm --prefix ui run check`, `npm --prefix ui run test:ui`, `cargo test --workspace`.
Then `OMACAL_SEED_DEMO=1 cargo tauri dev` — the demo fixture has two calendars, so the popover has something real to show. Confirm unticking one makes its events vanish immediately and reticking brings them back.

- [ ] **Step 7: Commit**

```bash
git add ui
git commit -m "feat(ui): calendar popover with show/hide and add/remove"
```

---

### Task 6: Connect more than one account

The sync loop already iterates every account and the schema has been multi-account since Plan 1. The only thing missing is a control.

**Files:**
- Modify: `ui/src/lib/Header.svelte`, `ui/src/App.svelte`, `ui/tests/components.spec.ts`

**Interfaces:**
- Consumes: the existing `sign_in` command and `AppStatus.accounts`
- Produces: no new backend surface

- [ ] **Step 1: Write the failing specs**

```ts
// ui/tests/components.spec.ts — in the Header describe block

test('a connected account can add another', async ({ page }) => {
  await page.goto(show('Header', 'connected'));
  await expect(page.getByRole('button', { name: 'Add account' })).toBeVisible();
});

test('a disconnected user is asked to connect, not to add', async ({ page }) => {
  await page.goto(show('Header', 'disconnected'));
  await expect(page.getByRole('button', { name: /Connect Google Calendar/ })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Add account' })).toHaveCount(0);
});

test('demo mode offers neither', async ({ page }) => {
  await page.goto(show('Header', 'demo'));
  await expect(page.getByRole('button', { name: 'Add account' })).toHaveCount(0);
  await expect(page.getByRole('button', { name: /Connect/ })).toHaveCount(0);
});
```

- [ ] **Step 2: Run to verify failure**

Run: `npm --prefix ui run test:ui -- --project=chromium -g "add another"`
Expected: FAIL — no such button.

- [ ] **Step 3: Implement**

In `Header.svelte`, the connected branch gains an **Add account** button beside Sync now, calling the same `onSignIn` handler. Keep it hidden in demo mode alongside the sync control — demo mode reaches neither Google nor the keyring.

```svelte
{#if connected}
  <span class="synced">{busy ? 'Syncing…' : `Synced ${relativeTime(status!.last_sync_ms)}`}</span>
  {#if !status?.demo}
    <button onclick={onSync} disabled={busy}>Sync now</button>
    <button onclick={onSignIn} disabled={busy}>Add account</button>
  {/if}
{:else}
  <button class="primary" onclick={onSignIn} disabled={busy}>
    {busy ? 'Connecting…' : 'Connect Google Calendar'}
  </button>
{/if}
```

- [ ] **Step 4: Refresh calendars after a second sign-in**

`handleSignIn` in `App.svelte` already refreshes status and syncs; add a calendar-list reload so the new account's calendars appear in the popover without a restart.

- [ ] **Step 5: Verify**

Run: `npm --prefix ui run test:ui`, `npm --prefix ui run check`.
Expected: green on both engines; state the new total.

- [ ] **Step 6: Commit**

```bash
git add ui
git commit -m "feat(ui): add a second Google account from the header"
```

---

### Task 7: Offer the calendar picker after every sign-in

Sign-in currently imports everything and shows it. A real account brought in four calendars including subscribed holidays; the first thing you see should not be a wall of events you did not ask for. That is just as true of a work account added to a personal one — arguably more so, since that is where shared and room calendars live.

So the picker opens after **every** successful sign-in, not only the first. This replaces the earlier `is_first_account` design: distinguishing the first account from later ones added a Rust helper, a command return-type change, and a UI branch, all to *suppress* the prompt exactly when the flood is largest. Opening it every time is both the simpler code and the better behaviour. The user chose this explicitly over first-run-only and over defaulting non-primary calendars to off.

Calendars still arrive `sync_enabled = 1, selected = 1`. Nothing is hidden from you by default; the picker just puts the choice in front of you while the import is fresh.

**Files:**
- Modify: `ui/src/App.svelte`, `ui/src/lib/CalendarPopover.svelte`, `ui/tests/components.spec.ts`, `ui/tests/fixtures.ts`

**Interfaces:**
- Consumes: the existing `sign_in` command, unchanged. No new backend surface — this task is UI only.
- Produces: `CalendarPopover` gains a bindable `open` prop so a parent can open it.

- [ ] **Step 1: Write the failing spec**

```ts
// ui/tests/components.spec.ts — in the CalendarPopover describe block

test('a parent can open the picker', async ({ page }) => {
  await page.goto(show('open-on-mount'));
  await expect(page.locator('.panel')).toBeVisible();
});

test('it still starts closed by default', async ({ page }) => {
  await page.goto(show('two-accounts'));
  await expect(page.locator('.panel')).toHaveCount(0);
});
```

Add an `open-on-mount` fixture that passes `open: true` alongside the existing calendar list.

- [ ] **Step 2: Run to verify failure**

Run: `npm --prefix ui run test:ui -- --project=chromium -g "parent can open"`
Expected: FAIL — the panel is not visible, because `open` is component-local state today.

- [ ] **Step 3: Make `open` bindable**

In `CalendarPopover.svelte`, promote `open` from plain `$state` to a bindable prop with a default, so the component still owns it when nobody binds:

```svelte
let {
  calendars,
  onchange,
  open = $bindable(false),
}: {
  calendars: Calendar[];
  onchange: () => void;
  open?: boolean;
} = $props();
```

Everything already assigning `open` — `toggle()`, `close()`, the scrim, the Escape handler — keeps working unchanged; `$bindable` writes propagate to the parent when bound and stay local when not.

The `message = null` reset currently lives in `toggle()`. Move it into an `$effect` keyed on `open` so a parent-driven open clears a stale note too:

```svelte
$effect(() => {
  if (open) message = null;
});
```

- [ ] **Step 4: Open it after a sign-in**

In `App.svelte`, `handleSignIn` already refreshes status, syncs, and (as of Task 6) reloads the calendar list. After that reload resolves, open the picker:

```svelte
let pickerOpen = $state(false);
// …inside handleSignIn, after the calendar list has loaded:
pickerOpen = true;
```

Bind it through the header: `<CalendarPopover {calendars} onchange={…} bind:open={pickerOpen} />`.

Order matters — open it only *after* the calendars have loaded, or the panel appears empty for a beat and then fills.

- [ ] **Step 5: Verify the sign-in path**

Add a spec driving the real path rather than only the prop:

```ts
test('signing in opens the picker with the new calendars in it', async ({ page }) => {
  await page.goto('/tests/harness/index.html?c=App&f=sign-in-adds-account');
  await page.getByRole('button', { name: /Connect|Add account/ }).click();
  await expect(page.locator('.panel')).toBeVisible();
  await expect(page.locator('.acct')).toHaveCount(1);
});
```

The fixture stubs `sign_in` to resolve and `get_calendars` to return the new account's calendars.

- [ ] **Step 6: Verify**

Run: `npm --prefix ui run test:ui`, `npm --prefix ui run check`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`.

- [ ] **Step 7: Commit**

```bash
git add ui
git commit -m "feat(ui): offer the calendar picker after every sign-in"
```

---


## Definition of Done

- [ ] Hiding a calendar removes it from the grid immediately and does not stop it syncing
- [ ] Removing a calendar deletes its local events, survives the next sync, and can be undone
- [ ] The popover groups by account and shows per-calendar colours
- [ ] A second Google account can be connected from the header, and both sync
- [ ] The calendar picker opens after every sign-in — first account or fifth — so a newly imported set of calendars is never silently switched on behind your back
- [ ] Event blocks show `Zoom` / `Google Meet` / a host, never a truncated URL
- [ ] A malformed `config.toml` cannot render the client secret in the UI
- [ ] `cargo test --workspace` ≥ 175, `npm --prefix ui run test:ui` ≥ 80, clippy and `check` clean
