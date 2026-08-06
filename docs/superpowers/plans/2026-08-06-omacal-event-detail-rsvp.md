# Event Detail and RSVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Click an event to see its description, guest list and responses, then answer Yes / Maybe / No without leaving the app.

**Architecture:** Sync stops discarding the detail fields Google already sends, so the popover paints from SQLite instantly and works offline; opening it also refreshes that one event in the background. RSVP is a `PATCH` carrying the whole attendee array with only your own response changed, guarded by `If-Match` and retried once on `412`.

**Tech Stack:** Rust (sqlx/SQLite, reqwest, wiremock), Svelte 5 runes + TypeScript, Tauri v2, Playwright.

**Spec:** `docs/superpowers/specs/2026-08-06-omacal-event-detail-rsvp-design.md`
**Base:** `main` @ `98dfcd7` — 192 Rust tests, 128 UI tests.

## Global Constraints

- **`selected` means displayed. `sync_enabled` means fetched.** No code may use one for the other's purpose.
- Time is `i64` epoch milliseconds. `chrono` stays confined to `crates/omacal-core`; `jiff` elsewhere.
- **Never `{:?}`-log, print, or interpolate a `Tokens` value or any token string.** `Tokens` has a hand-written redacting `Debug` at `auth.rs:66-77` — do not replace it with a derive.
- **The CSRF check in `sign_in` must not be removed or weakened.** It has no automated coverage.
- **Use `sqlx::query`/`query_as`/`query_scalar` (runtime-checked), never the `query!` macros** — they need `DATABASE_URL` at compile time.
- **Demo mode must never write to the real database or reach Google.** Its three enforcement points (separate DB, `demo_sync_guard`, `should_sync`/`may_sync`) all stay.
- **Never render event text with `{@html}`.** Descriptions are attacker-controlled: anyone who knows the user's email can put an event on their calendar.
- Svelte 5 runes only — `$props()`, `$state()`, `$derived()`, `$effect()`, `$bindable()`. No `export let`, no `$:`.
- **No live network calls in tests.** Rust uses `wiremock`; UI uses the harness stubs.
- **Never touch** `~/Library/Application Support/com.omacal.app/omacal.db` or `~/.config/omacal/config.toml`.
- `cargo test --workspace` starts at **192** and must never regress. `npm --prefix ui run test:ui` starts at **128**.

## Shared fixtures

Verified values — do not recompute, and do not substitute your own:

| Name | Value | Is |
|---|---|---|
| `MON_0900` | `1786341600000` | Monday 2026-08-10 09:00 Europe/Sofia (06:00Z) |
| `MON_0930` | `1786343400000` | Monday 2026-08-10 09:30 Europe/Sofia (06:30Z) |
| `THU_0900` | `1786600800000` | Thursday 2026-08-13 09:00 Europe/Sofia (06:00Z) |

## File structure

| File | Responsibility |
|---|---|
| `crates/omacal-store/migrations/0003_attendees.sql` | one column + cursor drop |
| `crates/omacal-store/src/events.rs` | `Attendee`, `StoredEvent` fields, upsert/read |
| `crates/omacal-sync/src/lib.rs` | map Google's fields instead of dropping them |
| `crates/omacal-google/src/client.rs` | `get_event`, `patch_event`, `event_instances` |
| `src-tauri/src/events.rs` | the commands, `can_respond`, `target_event_id`, the attendee-array builder |

**Why the attendee builder lives in `src-tauri` and not in `omacal-google`:** it maps `omacal_store::Attendee` into Google's JSON shape, so it touches both crates — and `omacal-google` deliberately does not depend on `omacal-store` (the API client and the persistence layer are independent; `sync` and `src-tauri` are what compose them). Putting it in the client crate would force that dependency backwards. `src-tauri` already depends on both.
| `ui/src/lib/sanitize.ts` | description → safe segments, never HTML |
| `ui/src/lib/position.ts` | pure flip/clamp geometry |
| `ui/src/lib/eventdetail.ts` | command bindings |
| `ui/src/lib/EventPopover.svelte` | the panel |
| `ui/src/lib/EventBlock.svelte` | click → anchor rect |
| `ui/src/lib/WeekGrid.svelte` | holds selection, renders popover |

---

### Task 1: Store the detail fields

The columns `description`, `etag`, `sequence` and `organizer_email` already exist in `0001_init.sql` and have never been written. Only `attendees_json` is new.

**Files:**
- Create: `crates/omacal-store/migrations/0003_attendees.sql`
- Modify: `crates/omacal-store/src/events.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct Attendee {
      pub email: String,
      pub display_name: Option<String>,
      pub response_status: String,
      pub optional: bool,
      pub is_self: bool,
  }
  // StoredEvent gains:
  //   pub description: Option<String>,
  //   pub etag: Option<String>,
  //   pub sequence: i64,
  //   pub organizer_email: Option<String>,
  //   pub attendees: Vec<Attendee>,
  ```

- [ ] **Step 1: Write the migration**

```sql
-- crates/omacal-store/migrations/0003_attendees.sql

-- A JSON column rather than a child table. Nothing queries attendees
-- independently, and `upsert_event` must stay a single statement: since the
-- Plan 1c race fix it runs inside `apply()`'s BEGIN IMMEDIATE transaction, and
-- a child table would drag attendee writes into that transaction.
ALTER TABLE events ADD COLUMN attendees_json TEXT;

-- The backfill. `description`, `etag`, `sequence` and `organizer_email` have
-- existed since 0001 and were never written, so every row already stored is
-- missing data the popover needs. Dropping every cursor makes the next sync a
-- full window fetch, which is the only way those rows acquire it. Costs one
-- slow sync on first launch after the update.
DELETE FROM sync_state;
```

- [ ] **Step 2: Write the failing test**

```rust
// crates/omacal-store/src/events.rs — tests module

#[tokio::test]
async fn attendees_round_trip_through_the_store() {
    let pool = crate::connect_memory().await.unwrap();
    let cal = seed(&pool).await;

    let ev = StoredEvent {
        id: 0,
        calendar_id: cal,
        google_id: "ev1".into(),
        summary: Some("Weekly Standup".into()),
        location: None,
        start_utc: 1786341600000,
        end_utc: 1786343400000,
        start_tz: "Europe/Sofia".into(),
        end_tz: "Europe/Sofia".into(),
        is_all_day: false,
        recurrence: None,
        recurring_event_id: None,
        original_start_utc: None,
        status: "confirmed".into(),
        self_response: Some("needsAction".into()),
        conference_uri: None,
        color_hex: None,
        description: Some("Sprint sync.".into()),
        etag: Some("\"etag-1\"".into()),
        sequence: 3,
        organizer_email: Some("ana@x.com".into()),
        attendees: vec![
            Attendee { email: "ana@x.com".into(), display_name: Some("Ana".into()),
                       response_status: "accepted".into(), optional: false, is_self: false },
            Attendee { email: "me@x.com".into(), display_name: None,
                       response_status: "needsAction".into(), optional: true, is_self: true },
        ],
    };
    upsert_event(&pool, &ev).await.unwrap();

    let back = events_in_window(&pool, 1786300000000, 1786400000000).await.unwrap();
    let got = back.iter().find(|e| e.google_id == "ev1").expect("event stored");

    assert_eq!(got.description.as_deref(), Some("Sprint sync."));
    assert_eq!(got.etag.as_deref(), Some("\"etag-1\""));
    assert_eq!(got.sequence, 3);
    assert_eq!(got.organizer_email.as_deref(), Some("ana@x.com"));
    assert_eq!(got.attendees.len(), 2, "attendees lost in the round trip");
    assert_eq!(got.attendees[1].email, "me@x.com");
    assert!(got.attendees[1].is_self, "the self flag must survive");
    assert!(got.attendees[1].optional, "the optional flag must survive");
    assert_eq!(got.attendees[0].display_name.as_deref(), Some("Ana"));
}

#[tokio::test]
async fn an_event_with_no_attendees_reads_back_as_an_empty_list() {
    // A NULL column must not become a parse error. Most personal events have
    // no guests at all, so this is the common path, not an edge case.
    let pool = crate::connect_memory().await.unwrap();
    let cal = seed(&pool).await;
    sqlx::query(
        "INSERT INTO events (calendar_id, google_id, start_utc, end_utc,
             start_tz, end_tz, status, updated_at)
         VALUES (?1, 'bare', 1786341600000, 1786343400000,
                 'Europe/Sofia', 'Europe/Sofia', 'confirmed', 0)")
        .bind(cal).execute(&pool).await.unwrap();

    let back = events_in_window(&pool, 1786300000000, 1786400000000).await.unwrap();
    let got = back.iter().find(|e| e.google_id == "bare").unwrap();
    assert!(got.attendees.is_empty());
    assert_eq!(got.sequence, 0);
}
```

```rust
#[tokio::test]
async fn the_migration_drops_every_sync_cursor_so_old_rows_get_backfilled() {
    // The four other columns have existed since 0001 and were never written,
    // so rows already on disk are missing data the popover needs. Dropping the
    // cursors is the entire backfill mechanism — without it those rows keep
    // their gaps until Google happens to send each event again.
    let pool = crate::connect_memory().await.unwrap();
    let cal = seed(&pool).await;
    sqlx::query("INSERT INTO sync_state (calendar_id, sync_token, window_start, window_end)
                 VALUES (?1, 'tok-from-before-the-upgrade', 0, 0)")
        .bind(cal).execute(&pool).await.unwrap();

    // `connect_memory` has already run every migration including 0003, so a
    // cursor inserted after the fact proves nothing. Re-run the statement the
    // migration performs and assert on its effect.
    sqlx::query("DELETE FROM sync_state").execute(&pool).await.unwrap();

    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sync_state")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(left, 0, "a surviving cursor means old rows never get their attendees");
}
```

**Note the honesty problem in that test** and do not paper over it: because migrations all run at connect time, a test cannot observe the migration acting on pre-existing rows. It asserts the statement's effect, not the migration's ordering. Say so in the test comment (as above) rather than naming it something that implies more than it checks. If you can find a way to run migrations `0001..0002`, insert, then apply `0003` — do that instead and delete this note.

- [ ] **Step 3: Run to verify failure**

Run: `cargo test -p omacal-store attendees_round_trip`
Expected: FAIL — `StoredEvent` has no field `attendees`.

- [ ] **Step 4: Add the type and the fields**

```rust
/// One invitee. Mirrors Google's `attendees[]` entry, kept in a JSON column
/// rather than a table — see 0003_attendees.sql for why.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Attendee {
    pub email: String,
    pub display_name: Option<String>,
    /// `accepted` | `declined` | `tentative` | `needsAction`.
    pub response_status: String,
    pub optional: bool,
    /// True for the signed-in user's own row. This is the entry an RSVP edits,
    /// and the only one it may edit.
    pub is_self: bool,
}
```

Add to `StoredEvent`: `description: Option<String>`, `etag: Option<String>`, `sequence: i64`, `organizer_email: Option<String>`, `attendees: Vec<Attendee>`.

- [ ] **Step 5: Write them in `upsert_event`**

Extend the `INSERT` column list with `description, etag, sequence, organizer_email, attendees_json` and bind five more parameters (the existing sixteen become twenty-one). Serialize with `serde_json::to_string(&ev.attendees)?`. Add all five to the `ON CONFLICT ... DO UPDATE SET` list so a re-sync refreshes them like any other field.

- [ ] **Step 6: Read them back**

In `events_in_window`'s `SELECT`, add the five columns, and build them in the row mapper. Attendees parse defensively — a malformed or absent JSON column yields an empty list rather than failing the whole window query:

```rust
attendees: r.get::<Option<String>, _>("attendees_json")
    .and_then(|s| serde_json::from_str(&s).ok())
    .unwrap_or_default(),
```

- [ ] **Step 7: Fix every other `StoredEvent` construction**

`cargo check --workspace` will name them. Set `description: None`, `etag: None`, `sequence: 0`, `organizer_email: None`, `attendees: Vec::new()` at each, except where a test needs otherwise.

- [ ] **Step 8: Verify**

Run: `cargo test --workspace`
Expected: PASS, count risen from 192 by the two new tests.

- [ ] **Step 9: Prove the tests guard**

Delete `attendees_json` from the `ON CONFLICT ... DO UPDATE SET` list, re-run: `attendees_round_trip_through_the_store` must still pass (first insert) — so **also** assert the update path by upserting twice with different attendees in that test before you finish. Then delete the column from the `INSERT` list and confirm the test fails. Restore. Put the transcript in your report.

- [ ] **Step 10: Commit**

```bash
git add crates/omacal-store
git commit -m "feat(store): store event description, etag, sequence and attendees"
```

---

### Task 2: Stop discarding them during sync

`crates/omacal-sync` builds `StoredEvent` from Google's `model::Event` and never reads `description`, `etag`, `sequence`, `organizer` or `attendees`, though all are parsed.

**Files:**
- Modify: `crates/omacal-sync/src/lib.rs`
- Modify: `crates/omacal-google/src/model.rs` (add `organizer`)

**Interfaces:**
- Consumes: `omacal_store::Attendee`, the new `StoredEvent` fields from Task 1.

- [ ] **Step 1: Add `organizer` to the Google model**

```rust
// crates/omacal-google/src/model.rs
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Organizer {
    #[serde(default)]
    pub email: String,
    pub display_name: Option<String>,
}
```

Add to `Event`: `#[serde(default)] pub organizer: Organizer,`.

- [ ] **Step 2: Write the failing test**

```rust
// crates/omacal-sync/src/lib.rs — tests module

#[tokio::test]
async fn a_synced_event_carries_its_guest_list_and_description() {
    // The fields arrive on every sync already; this pins that they are stored
    // rather than parsed and dropped.
    let server = wiremock::MockServer::start().await;
    mount_events_page(&server, serde_json::json!({
        "items": [{
            "id": "ev1",
            "status": "confirmed",
            "etag": "\"etag-1\"",
            "summary": "Weekly Standup",
            "description": "Sprint sync.",
            "sequence": 3,
            "organizer": { "email": "ana@x.com", "displayName": "Ana" },
            "start": { "dateTime": "2026-08-10T09:00:00+03:00", "timeZone": "Europe/Sofia" },
            "end":   { "dateTime": "2026-08-10T09:30:00+03:00", "timeZone": "Europe/Sofia" },
            "attendees": [
                { "email": "ana@x.com", "displayName": "Ana", "responseStatus": "accepted" },
                { "email": "me@x.com", "responseStatus": "needsAction", "self": true, "optional": true }
            ]
        }],
        "nextSyncToken": "tok-1"
    })).await;

    let (pool, cal) = seed_pool_and_calendar().await;
    let client = omacal_google::CalendarClient::new(server.uri(), "at");
    sync_calendar(&pool, &client, cal, "cal@x.com", 1786300000000, 1786400000000)
        .await.unwrap();

    let stored = omacal_store::events_in_window(&pool, 1786300000000, 1786400000000)
        .await.unwrap();
    let ev = stored.iter().find(|e| e.google_id == "ev1").unwrap();

    assert_eq!(ev.description.as_deref(), Some("Sprint sync."));
    assert_eq!(ev.etag.as_deref(), Some("\"etag-1\""));
    assert_eq!(ev.sequence, 3);
    assert_eq!(ev.organizer_email.as_deref(), Some("ana@x.com"));
    assert_eq!(ev.attendees.len(), 2, "guest list dropped during sync");
    assert!(ev.attendees.iter().any(|a| a.is_self && a.optional));
}
```

If `mount_events_page` and `seed_pool_and_calendar` do not exist under those names, use whatever the existing sync tests use — read the tests module first and follow it rather than inventing helpers.

- [ ] **Step 3: Run to verify failure**

Run: `cargo test -p omacal-sync guest_list`
Expected: FAIL — `description` is `None`.

- [ ] **Step 4: Map the fields**

Where the `StoredEvent` is built from `model::Event`, populate the five fields. Attendees map straight across:

```rust
attendees: e.attendees.iter().map(|a| omacal_store::Attendee {
    email: a.email.clone(),
    display_name: a.display_name.clone(),
    response_status: a.response_status.clone(),
    optional: a.optional,
    is_self: a.is_self,
}).collect(),
organizer_email: (!e.organizer.email.is_empty()).then(|| e.organizer.email.clone()),
```

- [ ] **Step 5: Verify and prove the test guards**

Run `cargo test --workspace`. Then delete the `attendees:` mapping line, confirm the new test fails, restore. Transcript in the report.

- [ ] **Step 6: Commit**

```bash
git add crates/omacal-sync crates/omacal-google
git commit -m "feat(sync): keep description, etag, organizer and attendees"
```

---

### Task 3: The detail command

**Files:**
- Create: `src-tauri/src/events.rs`
- Modify: `src-tauri/src/lib.rs` (declare the module, register the command)

**Interfaces:**
- Produces:
  ```rust
  #[derive(serde::Serialize)]
  pub struct EventDetail {
      pub id: i64,
      pub title: Option<String>,
      pub description: Option<String>,
      pub location: Option<String>,
      pub conference_uri: Option<String>,
      pub start_ms: i64,
      pub end_ms: i64,
      pub is_all_day: bool,
      pub is_recurring: bool,
      pub color: Option<String>,
      pub organizer_email: Option<String>,
      pub self_response: Option<String>,
      pub can_respond: bool,
      pub attendees: Vec<omacal_store::Attendee>,
  }
  pub(crate) fn can_respond(access_role: &str, attendees: &[omacal_store::Attendee]) -> bool;
  // #[tauri::command] event_detail(id: i64) -> Result<EventDetail, String>
  ```

- [ ] **Step 1: Write the failing tests**

```rust
// src-tauri/src/events.rs — tests module
use omacal_store::Attendee;

fn guest(is_self: bool) -> Attendee {
    Attendee { email: "me@x.com".into(), display_name: None,
               response_status: "needsAction".into(), optional: false, is_self }
}

#[test]
fn a_writable_calendar_where_you_are_a_guest_can_respond() {
    assert!(can_respond("owner", &[guest(true)]));
    assert!(can_respond("writer", &[guest(true)]));
}

#[test]
fn a_read_only_calendar_cannot_respond_however_many_guests() {
    // A subscribed holiday calendar, or one shared with you read-only. The
    // buttons are hidden rather than disabled: a disabled control invites a
    // click and explains nothing.
    assert!(!can_respond("reader", &[guest(true)]));
    assert!(!can_respond("freeBusyReader", &[guest(true)]));
}

#[test]
fn an_event_you_are_not_invited_to_cannot_be_answered() {
    // Watching someone else's calendar you have write access to. There is no
    // attendee row of yours to change, and patching would rewrite theirs.
    let others = vec![Attendee { email: "ana@x.com".into(), display_name: None,
                                 response_status: "accepted".into(),
                                 optional: false, is_self: false }];
    assert!(!can_respond("owner", &others));
    assert!(!can_respond("owner", &[]));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p omacal can_respond`
Expected: FAIL — `cannot find function can_respond`.

- [ ] **Step 3: Implement**

```rust
/// Whether the RSVP controls are shown at all.
///
/// Two independent reasons to withhold them: the calendar is not writable, or
/// there is no attendee row of yours to change. The second matters as much as
/// the first — an RSVP patch rewrites the whole attendee array, so without a
/// `self` row there is nothing to edit and everything to damage.
pub(crate) fn can_respond(access_role: &str, attendees: &[omacal_store::Attendee]) -> bool {
    matches!(access_role, "owner" | "writer") && attendees.iter().any(|a| a.is_self)
}
```

Then the command: read the event and its calendar's `access_role` by id, build `EventDetail`, set `is_recurring` from `recurrence.is_some() || recurring_event_id.is_some()`, and map errors through `crate::errors::user_facing`.

Add a store helper `omacal_store::event_by_id(pool, id) -> anyhow::Result<Option<(StoredEvent, String)>>` returning the event and its calendar's `access_role`, joined. Follow `list_calendars`' shape.

- [ ] **Step 4: Verify**

Run: `cargo test --workspace` — three new tests pass.

- [ ] **Step 5: Prove they guard**

Change `can_respond` to ignore `access_role` entirely (`attendees.iter().any(..)`). Confirm `a_read_only_calendar_cannot_respond_however_many_guests` fails. Restore. Then drop the `is_self` requirement and confirm `an_event_you_are_not_invited_to_cannot_be_answered` fails. Restore. Transcript in the report.

- [ ] **Step 6: Commit**

```bash
git add src-tauri crates/omacal-store
git commit -m "feat(app): event_detail command and RSVP eligibility"
```

---

### Task 4: Google write methods

**Files:**
- Modify: `crates/omacal-google/src/client.rs`

**Interfaces:**
- Produces:
  ```rust
  pub async fn get_event(&self, cal: &str, event_id: &str)
      -> Result<model::Event, ApiError>;
  pub async fn patch_event(&self, cal: &str, event_id: &str,
      body: &serde_json::Value, etag: Option<&str>)
      -> Result<model::Event, ApiError>;
  pub async fn event_instances(&self, cal: &str, event_id: &str,
      time_min: &str, time_max: &str)
      -> Result<Vec<model::Event>, ApiError>;
  ```
  `ApiError` gains `#[error("the event changed while you were editing it")] PreconditionFailed`.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/omacal-google/src/client.rs — tests module

#[tokio::test]
async fn a_patch_asks_google_to_tell_the_organiser() {
    // events.patch notifies nobody by default. An RSVP the organiser never
    // receives is worse than none: the user believes they have declined and
    // the organiser is still expecting them.
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("PATCH"))
        .and(wiremock::matchers::query_param("sendUpdates", "all"))
        .respond_with(wiremock::ResponseTemplate::new(200)
            .set_body_json(serde_json::json!({ "id": "ev1", "status": "confirmed" })))
        .expect(1)
        .mount(&server).await;

    let c = CalendarClient::new(server.uri(), "at");
    c.patch_event("cal@x.com", "ev1", &serde_json::json!({}), None).await.unwrap();
    // `.expect(1)` fails the test on drop if sendUpdates=all was absent.
}

#[tokio::test]
async fn a_stale_etag_surfaces_as_precondition_failed() {
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("PATCH"))
        .respond_with(wiremock::ResponseTemplate::new(412))
        .mount(&server).await;

    let c = CalendarClient::new(server.uri(), "at");
    let err = c.patch_event("cal@x.com", "ev1", &serde_json::json!({}), Some("\"old\""))
        .await.unwrap_err();
    assert!(matches!(err, ApiError::PreconditionFailed), "got {err:?}");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p omacal-google sendUpdates`
Expected: FAIL — `no method named patch_event`.

- [ ] **Step 3: Implement the client methods**

`get_event` → `GET {base}/calendars/{cal}/events/{id}`, url-encoding both path segments.
`patch_event` → `PATCH {base}/calendars/{cal}/events/{id}?sendUpdates=all`, `If-Match` when `etag` is `Some`, mapping `412` to `ApiError::PreconditionFailed`.
`event_instances` → `GET {base}/calendars/{cal}/events/{id}/instances?timeMin=&timeMax=`, returning `items`.

Follow `list_events`' existing error mapping for everything else; do not invent a second style.

- [ ] **Step 4: Verify and prove the tests guard**

`cargo test --workspace`. Then drop `sendUpdates=all` from the query and confirm `a_patch_asks_google_to_tell_the_organiser` fails; map `412` to `ApiError::Http` instead and confirm `a_stale_etag_surfaces_as_precondition_failed` fails. Restore each. Transcript in the report.

- [ ] **Step 5: Commit**

```bash
git add crates/omacal-google
git commit -m "feat(google): get_event, patch_event and instance lookup"
```

---

### Task 5: The respond command

**Files:**
- Modify: `src-tauri/src/events.rs`, `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `CalendarClient::{get_event, patch_event, event_instances}`, `can_respond` from Task 3.
- Produces:
  ```rust
  pub(crate) fn attendees_with_self_response(
      attendees: &[omacal_store::Attendee], response: &str,
  ) -> Option<Vec<serde_json::Value>>;   // None when no attendee is `self`
  pub(crate) fn target_event_id(
      scope: &str, recurring_event_id: Option<&str>, own_id: &str,
  ) -> Target;
  // #[tauri::command]
  // respond_to_event(id: i64, response: String, scope: String) -> Result<EventDetail, String>
  // scope is "this" | "all"
  // #[tauri::command] refresh_event(id: i64) -> Result<EventDetail, String>
  ```

- [ ] **Step 1: Write the failing tests**

The command itself reads the Keychain, so the two pieces worth testing are the pure ones: which attendee array gets sent, and which event id gets patched.

```rust
// src-tauri/src/events.rs — tests module
use omacal_store::Attendee;

fn three() -> Vec<Attendee> {
    vec![
        Attendee { email: "ana@x.com".into(), display_name: Some("Ana".into()),
                   response_status: "accepted".into(), optional: false, is_self: false },
        Attendee { email: "me@x.com".into(), display_name: None,
                   response_status: "needsAction".into(), optional: false, is_self: true },
        Attendee { email: "petya@x.com".into(), display_name: None,
                   response_status: "declined".into(), optional: true, is_self: false },
    ]
}

#[test]
fn responding_changes_only_your_own_row() {
    // Google replaces the attendee array wholesale on patch. Sending a list
    // that has quietly reset someone else's answer is the worst thing this
    // feature could do to a real calendar, so this is the load-bearing test.
    let out = attendees_with_self_response(&three(), "declined").unwrap();
    assert_eq!(out.len(), 3, "an attendee was dropped");
    assert_eq!(out[0]["email"], "ana@x.com");
    assert_eq!(out[0]["responseStatus"], "accepted", "Ana's answer was overwritten");
    assert_eq!(out[1]["email"], "me@x.com");
    assert_eq!(out[1]["responseStatus"], "declined");
    assert_eq!(out[2]["email"], "petya@x.com");
    assert_eq!(out[2]["responseStatus"], "declined", "Petya's answer was overwritten");
    assert_eq!(out[2]["optional"], true, "the optional flag was lost");
}

#[test]
fn without_a_self_row_there_is_nothing_to_answer() {
    let others: Vec<Attendee> = three().into_iter().filter(|a| !a.is_self).collect();
    assert!(attendees_with_self_response(&others, "accepted").is_none());
    assert!(attendees_with_self_response(&[], "accepted").is_none());
}

#[test]
fn answering_the_whole_series_targets_the_master() {
    // An exception row carries the series id; the master carries its own.
    assert_eq!(target_event_id("all", Some("master-1"), "instance-9"), Target::Master("master-1".into()));
    assert_eq!(target_event_id("all", None, "master-1"), Target::Master("master-1".into()));
}

#[test]
fn answering_one_occurrence_asks_google_which_instance_it_is() {
    // Instance ids look like `{master}_{20260813T060000Z}`, and formatting that
    // by hand works until an all-day event or an already-moved occurrence
    // breaks it silently. The caller must resolve it against the API instead.
    assert_eq!(
        target_event_id("this", Some("master-1"), "instance-9"),
        Target::Instance { master: "master-1".into(), fallback: "instance-9".into() }
    );
}

#[test]
fn a_one_off_event_is_patched_directly_whatever_the_scope() {
    // No recurrence anywhere: both scopes mean the same single event, and no
    // instance lookup should happen.
    assert_eq!(target_event_id("this", None, "ev1"), Target::Master("ev1".into()));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p omacal target_event_id`
Expected: FAIL — `cannot find function target_event_id`.

- [ ] **Step 3: Implement the attendee builder**

```rust
/// Rebuilds the attendee array with only the `self` row's response changed.
///
/// Every other attendee is copied through field for field. Google replaces the
/// list wholesale on patch, so anything omitted here is *erased* from the real
/// event — including other people's answers.
///
/// `None` when no attendee is marked `self`: there is no row of ours to edit,
/// and sending the list anyway would rewrite other people's for no reason.
pub(crate) fn attendees_with_self_response(
    attendees: &[omacal_store::Attendee],
    response: &str,
) -> Option<Vec<serde_json::Value>> {
    if !attendees.iter().any(|a| a.is_self) {
        return None;
    }
    Some(attendees.iter().map(|a| {
        let status = if a.is_self { response } else { a.response_status.as_str() };
        let mut v = serde_json::json!({
            "email": a.email,
            "responseStatus": status,
            "optional": a.optional,
        });
        if let Some(n) = &a.display_name {
            v["displayName"] = serde_json::Value::String(n.clone());
        }
        v
    }).collect())
}
```

- [ ] **Step 4: Implement the id mapping**

```rust
#[derive(Debug, PartialEq)]
pub(crate) enum Target {
    /// Patch this id directly.
    Master(String),
    /// Resolve the occurrence through `events.instances` first; `fallback` is
    /// the stored row's own id, used when the row is already a materialised
    /// exception and the lookup finds nothing.
    Instance { master: String, fallback: String },
}

/// Which Google event id an RSVP should patch.
pub(crate) fn target_event_id(
    scope: &str,
    recurring_event_id: Option<&str>,
    own_id: &str,
) -> Target {
    match (scope, recurring_event_id) {
        ("all", Some(master)) => Target::Master(master.to_string()),
        ("all", None) => Target::Master(own_id.to_string()),
        (_, Some(master)) => Target::Instance {
            master: master.to_string(),
            fallback: own_id.to_string(),
        },
        // Not recurring at all: one event, one id, no lookup.
        (_, None) => Target::Master(own_id.to_string()),
    }
}
```

A one-off event whose row *is* the master but which has `recurrence` set (a series master rendered directly) also takes the `Instance` path when scope is `this`; the caller passes `recurring_event_id.or(Some(own_id))` for rows carrying `recurrence`. Handle that at the call site, not inside this function — it stays a pure mapping.

- [ ] **Step 5: Wire the command**

```rust
#[tauri::command]
pub async fn respond_to_event(
    state: tauri::State<'_, AppState>,
    id: i64,
    response: String,
    scope: String,
) -> Result<EventDetail, String> {
    respond_impl(&state, id, &response, &scope)
        .await
        .map_err(|e| crate::errors::user_facing(&e))
}

async fn respond_impl(
    state: &AppState,
    id: i64,
    response: &str,
    scope: &str,
) -> anyhow::Result<EventDetail> {
    let (ev, access_role, cal_google_id, account_email) =
        omacal_store::event_for_write(&state.pool, id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("that event is no longer here"))?;

    if !can_respond(&access_role, &ev.attendees) {
        anyhow::bail!("this calendar cannot be answered from omacal");
    }
    let body_attendees = attendees_with_self_response(&ev.attendees, response)
        .ok_or_else(|| anyhow::anyhow!("you are not a guest on this event"))?;

    let cfg = crate::load_config()?;
    let token = crate::access_token_for(state, &cfg, &account_email).await?;
    let client = omacal_google::CalendarClient::new(crate::GOOGLE_CALENDAR_API, &token);

    // A row carrying `recurrence` is a series master; scope "this" must still
    // go through instance resolution for it, which is why `own_id` is offered
    // as the master when there is no `recurring_event_id`.
    let series = ev.recurring_event_id.as_deref()
        .or_else(|| ev.recurrence.as_ref().map(|_| ev.google_id.as_str()));
    let target = target_event_id(scope, series, &ev.google_id);

    let event_id = match &target {
        Target::Master(id) => id.clone(),
        Target::Instance { master, fallback } => {
            let found = client
                .event_instances(&cal_google_id, master,
                                 &crate::to_rfc3339(ev.start_utc - 1000),
                                 &crate::to_rfc3339(ev.start_utc + 1000))
                .await?;
            // Google's own id, never one built by string formatting: an
            // all-day event and an already-moved occurrence both format
            // differently, and getting it wrong patches the wrong day.
            found.first().map(|i| i.id.clone()).unwrap_or_else(|| fallback.clone())
        }
    };

    let body = serde_json::json!({ "attendees": body_attendees });
    let patched = match client
        .patch_event(&cal_google_id, &event_id, &body, ev.etag.as_deref()).await
    {
        Ok(p) => p,
        Err(omacal_google::ApiError::PreconditionFailed) => {
            // Someone edited the event while the popover was open. Re-read,
            // re-apply our answer to the list as it is now, and try once more —
            // retrying with the same stale list would overwrite their change.
            let fresh = client.get_event(&cal_google_id, &event_id).await?;
            let fresh_attendees: Vec<omacal_store::Attendee> =
                fresh.attendees.iter().map(crate::events::from_google_attendee).collect();
            let retry = attendees_with_self_response(&fresh_attendees, response)
                .ok_or_else(|| anyhow::anyhow!("you are not a guest on this event"))?;
            client.patch_event(&cal_google_id, &event_id,
                               &serde_json::json!({ "attendees": retry }),
                               fresh.etag.as_deref()).await?
        }
        Err(e) => return Err(e.into()),
    };

    // Close the loop locally: the week grid styles blocks from `self_response`,
    // so without this the block stays looking accepted until the next tick.
    // Straight through `upsert_event` — this is a direct user action, not sync,
    // and does not belong in `apply()`'s transaction.
    let mut row = ev;
    crate::events::merge_patched(&mut row, &patched);
    omacal_store::upsert_event(&state.pool, &row).await?;

    event_detail_impl(&state.pool, id).await
}
```

`refresh_event` is the same shape without the patch: `get_event`, `merge_patched`, `upsert_event`, return the detail. Its failures are the caller's to ignore.

Add the two small helpers this needs — `from_google_attendee` (a `model::Attendee` → `omacal_store::Attendee` map, the same one Task 2 wrote inline; lift it here and have Task 2's call site use it) and `merge_patched` (copy `etag`, `sequence`, `attendees`, and the derived `self_response` onto the stored row). Also add `omacal_store::event_for_write(pool, id)` returning `(StoredEvent, access_role, calendar google_id, account email)` — the same join as `event_by_id` from Task 3 with two more columns; extend that function rather than writing a second one.

- [ ] **Step 6: Verify and prove the tests guard**

`cargo test --workspace`. Then make `target_event_id` return `Master` for every input and confirm `answering_one_occurrence_asks_google_which_instance_it_is` fails; make the builder set `status` unconditionally (ignoring `is_self`) and confirm `responding_changes_only_your_own_row` fails. Restore each. Transcript in the report.

- [ ] **Step 7: Commit**

```bash
git add src-tauri
git commit -m "feat(app): respond_to_event with scope and conflict retry"
```

---

### Task 6: Descriptions are untrusted

**Files:**
- Create: `ui/src/lib/sanitize.ts`, `ui/tests/sanitize.spec.ts`

**Interfaces:**
- Produces:
  ```ts
  export type Segment = { kind: 'text' | 'link'; value: string };
  export function descriptionSegments(raw: string | null): Segment[];
  ```

Returning **segments, not an HTML string**, is the design. The component renders them with `{#each}` and a plain `<a>` — so there is no code path where `{@html}` could be reintroduced by a later edit.

- [ ] **Step 1: Write the failing tests**

```ts
// ui/tests/sanitize.spec.ts
import { test, expect } from '@playwright/test';
import { descriptionSegments } from '../src/lib/sanitize';

const text = (raw: string | null) =>
  descriptionSegments(raw).map((s) => s.value).join('');

test.describe('descriptionSegments', () => {
  test('a script tag is shown, not run', () => {
    // Anyone who knows your email can put an event on your calendar, so a
    // description is attacker-controlled input inside a webview that can call
    // Tauri commands. It must never become markup.
    const out = descriptionSegments('<script>alert(1)</script>');
    expect(out.every((s) => s.kind === 'text')).toBe(true);
    expect(text('<script>alert(1)</script>')).not.toContain('<script>');
  });

  test('an img onerror payload survives only as text', () => {
    expect(text('<img src=x onerror=alert(1)>')).not.toContain('onerror');
  });

  test('line breaks become newlines', () => {
    expect(text('one<br>two<br/>three')).toBe('one\ntwo\nthree');
    expect(text('<p>one</p><p>two</p>')).toBe('one\ntwo');
  });

  test('entities are decoded', () => {
    expect(text('Tom &amp; Jerry &lt;3 &quot;hi&quot; &#39;x&#39;&nbsp;y'))
      .toBe('Tom & Jerry <3 "hi" \'x\' y');
  });

  test('a bare url becomes a link segment', () => {
    const out = descriptionSegments('join at https://meet.google.com/abc now');
    expect(out.map((s) => s.kind)).toEqual(['text', 'link', 'text']);
    expect(out[1].value).toBe('https://meet.google.com/abc');
  });

  test('a javascript: url is never a link', () => {
    // The linkifier is the one place a URL becomes an href, so it is the one
    // place a scheme check belongs.
    expect(descriptionSegments('javascript:alert(1)').every((s) => s.kind === 'text'))
      .toBe(true);
  });

  test('null and empty give nothing to render', () => {
    expect(descriptionSegments(null)).toEqual([]);
    expect(descriptionSegments('   ')).toEqual([]);
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `npm --prefix ui run test:ui -- --project=chromium -g "script tag"`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement**

Order matters: convert breaks first, then strip tags, then decode entities (decoding first would let `&lt;script&gt;` become a real tag), then linkify with an `https?:` -only pattern. Collapse three or more newlines to two, and trim.

- [ ] **Step 4: Verify and prove the tests guard**

Run the file. Then move entity decoding *before* tag stripping and confirm a test fails; restore. Then allow any scheme in the linkifier and confirm the `javascript:` test fails; restore. Transcript in the report.

- [ ] **Step 5: Commit**

```bash
git add ui/src/lib/sanitize.ts ui/tests/sanitize.spec.ts
git commit -m "feat(ui): render event descriptions as text, never markup"
```

---

### Task 7: Popover placement

**Files:**
- Create: `ui/src/lib/position.ts`, `ui/tests/position.spec.ts`

**Interfaces:**
- Produces:
  ```ts
  export type Rect = { top: number; left: number; width: number; height: number };
  export type Size = { width: number; height: number };
  export function placePopover(
    anchor: Rect, popover: Size, viewport: Size, gap?: number
  ): { top: number; left: number };
  ```

Pure geometry, so it is testable without a browser and reusable by Day and Month views unchanged.

- [ ] **Step 1: Write the failing tests**

```ts
// ui/tests/position.spec.ts
import { test, expect } from '@playwright/test';
import { placePopover } from '../src/lib/position';

const VIEW = { width: 1200, height: 800 };
const POP = { width: 320, height: 400 };

test.describe('placePopover', () => {
  test('opens to the right of the anchor when there is room', () => {
    const p = placePopover({ top: 100, left: 200, width: 120, height: 40 }, POP, VIEW);
    expect(p.left).toBe(328); // 200 + 120 + 8
    expect(p.top).toBe(100);
  });

  test('flips to the left when it would run off the right edge', () => {
    const p = placePopover({ top: 100, left: 1000, width: 120, height: 40 }, POP, VIEW);
    expect(p.left).toBe(672); // 1000 - 320 - 8
  });

  test('clamps rather than flipping off the left edge too', () => {
    // A narrow viewport where neither side fits: stay on screen and overlap
    // rather than render half off it.
    const p = placePopover({ top: 100, left: 10, width: 40, height: 40 }, POP,
                           { width: 360, height: 800 });
    expect(p.left).toBeGreaterThanOrEqual(8);
    expect(p.left + POP.width).toBeLessThanOrEqual(360 - 8);
  });

  test('a low anchor lifts the popover to stay on screen', () => {
    const p = placePopover({ top: 700, left: 200, width: 120, height: 40 }, POP, VIEW);
    expect(p.top + POP.height).toBeLessThanOrEqual(800 - 8);
    expect(p.top).toBeGreaterThanOrEqual(8);
  });

  test('a popover taller than the viewport pins to the top', () => {
    const p = placePopover({ top: 300, left: 200, width: 120, height: 40 },
                           { width: 320, height: 900 }, VIEW);
    expect(p.top).toBe(8);
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `npm --prefix ui run test:ui -- --project=chromium -g "opens to the right"`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement**

```ts
export function placePopover(
  anchor: Rect, popover: Size, viewport: Size, gap = 8,
): { top: number; left: number } {
  // Prefer the right of the anchor; flip only when that would overflow, so the
  // popover sits on a consistent side for most events rather than jittering.
  const right = anchor.left + anchor.width + gap;
  let left = right + popover.width + gap > viewport.width
    ? anchor.left - popover.width - gap
    : right;

  // Clamp after flipping: in a viewport too narrow for either side, neither
  // choice fits and staying on screen beats being half off it.
  left = Math.min(left, viewport.width - popover.width - gap);
  left = Math.max(gap, left);

  // Top-aligned with the anchor, lifted only as far as needed. `max(gap, …)`
  // runs last so a popover taller than the viewport pins to the top edge
  // instead of going negative.
  let top = Math.min(anchor.top, viewport.height - popover.height - gap);
  top = Math.max(gap, top);

  return { top, left };
}
```

- [ ] **Step 4: Verify and prove the tests guard**

Run the file. Then remove the flip branch and confirm the flip test fails; restore. Then remove the vertical clamp and confirm the low-anchor test fails; restore. Transcript in the report.

- [ ] **Step 5: Commit**

```bash
git add ui/src/lib/position.ts ui/tests/position.spec.ts
git commit -m "feat(ui): popover placement with flip and clamp"
```

---

### Task 8: The popover, wired up

**Files:**
- Create: `ui/src/lib/eventdetail.ts`, `ui/src/lib/EventPopover.svelte`
- Modify: `ui/src/lib/EventBlock.svelte`, `ui/src/lib/WeekGrid.svelte`, `ui/tests/fixtures.ts`, `ui/tests/harness/mount.ts`, `ui/tests/harness/tauri.ts`, `ui/tests/components.spec.ts`
- Modify: `src-tauri/src/fixtures.rs` (demo events need guests)

**Interfaces:**
- Consumes: `descriptionSegments`, `placePopover`, and the three commands.
- Produces:
  ```ts
  export type Attendee = { email: string; display_name: string | null;
    response_status: string; optional: boolean; is_self: boolean };
  export type EventDetail = { id: number; title: string | null;
    description: string | null; location: string | null; conference_uri: string | null;
    start_ms: number; end_ms: number; is_all_day: boolean; is_recurring: boolean;
    color: string | null; organizer_email: string | null; self_response: string | null;
    can_respond: boolean; attendees: Attendee[] };
  export const getEventDetail: (id: number) => Promise<EventDetail>;
  export const refreshEvent: (id: number) => Promise<EventDetail>;
  export const respondToEvent:
    (id: number, response: string, scope: 'this' | 'all') => Promise<EventDetail>;
  ```

- [ ] **Step 1: Write the failing specs**

```ts
// ui/tests/components.spec.ts
test.describe('EventPopover', () => {
  const show = (f: string) => `/tests/harness/index.html?c=EventPopover&f=${f}`;

  test('shows the guest list with each response', async ({ page }) => {
    await page.goto(show('standup'));
    await expect(page.locator('.guest')).toHaveCount(3);
    await expect(page.locator('.guest.accepted')).toHaveCount(1);
    await expect(page.locator('.guest.declined')).toHaveCount(1);
  });

  test('a description containing markup is shown as text', async ({ page }) => {
    await page.goto(show('nasty-description'));
    await expect(page.locator('.desc')).toContainText('<script>alert(1)</script>');
    await expect(page.locator('.desc script')).toHaveCount(0);
  });

  test('a one-off event offers no scope choice', async ({ page }) => {
    await page.goto(show('standup'));
    await expect(page.locator('.rsvp')).toBeVisible();
    await expect(page.locator('.scope')).toHaveCount(0);
  });

  test('a recurring event asks which occurrences', async ({ page }) => {
    await page.goto(show('recurring'));
    await expect(page.locator('.scope')).toBeVisible();
    await expect(page.getByRole('radio', { name: /This one/ })).toBeChecked();
  });

  test('a read-only calendar offers no rsvp at all', async ({ page }) => {
    await page.goto(show('readonly'));
    await expect(page.locator('.guest')).toHaveCount(3);
    await expect(page.locator('.rsvp')).toHaveCount(0);
  });

  test('a failed response rolls the choice back and says why', async ({ page }) => {
    await page.goto(show('respond-fails'));
    await page.getByRole('button', { name: 'No' }).click();
    await expect(page.locator('.note.err')).toBeVisible();
    await expect(page.getByRole('button', { name: 'No' })).not.toHaveClass(/chosen/);
  });

  test('escape closes it even when focus has fallen to the body', async ({ page }) => {
    // Plan 1c shipped this bug once: a keydown handler on the panel misses
    // Escape entirely once a disabled control drops focus to <body>, and a
    // test that only presses Escape with the trigger focused cannot see it.
    await page.goto(show('standup'));
    await expect(page.locator('.pop')).toBeVisible();
    await page.evaluate(() => (document.activeElement as HTMLElement)?.blur());
    expect(await page.evaluate(() => document.activeElement?.tagName)).toBe('BODY');
    await page.keyboard.press('Escape');
    await expect(page.locator('.pop')).toHaveCount(0);
  });

  test('clicking a guest list does not close it', async ({ page }) => {
    // The scrim must sit behind the panel, not over it.
    await page.goto(show('standup'));
    await page.locator('.guest').first().click();
    await expect(page.locator('.pop')).toBeVisible();
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `npm --prefix ui run test:ui -- --project=chromium -g "guest list"`
Expected: FAIL — no such component.

- [ ] **Step 3: Build the component**

`EventPopover.svelte` takes `{ detail, anchor, onclose }`. Position with `placePopover` against `window.innerWidth/innerHeight`, measured after mount. Render description via `descriptionSegments` — `{#each}` over segments, `<a href={s.value} rel="noopener noreferrer" target="_blank">` for links, plain text otherwise. **No `{@html}` anywhere in this file.**

RSVP: three buttons plus, when `is_recurring`, a two-radio `.scope` group defaulting to `this`. Optimistic — mark the chosen response immediately, call `respondToEvent`, and on rejection restore the previous value and show `.note.err` naming the failure. This is the pattern `CalendarPopover` established; follow it rather than inventing a second one.

Escape and click-away reuse `CalendarPopover`'s approach: `<svelte:window onkeydown>` gated on being open (a handler on the panel misses keystrokes once focus falls to `<body>`) plus a sibling scrim.

- [ ] **Step 4: Open it from the grid**

`EventBlock` calls `onopen(event, rect)` with `event.currentTarget.getBoundingClientRect()`. `WeekGrid` holds `selected` and `anchor` in `$state`, loads detail with `getEventDetail`, renders the popover, and fires `refreshEvent` after paint — updating in place if it differs, and **ignoring failures silently**, since it is a freshness optimisation and not a load.

- [ ] **Step 5: Give demo mode guests**

Add attendees and a description to at least one seeded demo event in `src-tauri/src/fixtures.rs`, so the popover is exercisable with `OMACAL_SEED_DEMO=1` and no Google account. Include one `accepted`, one `declined`, one `needsAction`, and mark one `self`.

- [ ] **Step 6: Verify**

Run: `npm --prefix ui run check` (0 errors), `npm --prefix ui run test:ui`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`.

- [ ] **Step 7: Prove the tests guard**

Break each and confirm the matching spec fails, restoring after each: render the description with `{@html}` instead of segments (the markup test must fail); drop the `is_recurring` condition so `.scope` always renders (the one-off test must fail); remove the rollback in the catch (the failed-response test must fail). Transcript in the report.

- [ ] **Step 8: Commit**

```bash
git add ui src-tauri
git commit -m "feat(ui): event detail popover with RSVP"
```

---

## Definition of Done

- [ ] Clicking an event opens a popover with description, guest list and responses, conference link, and location
- [ ] The popover paints from local data and updates if the background refresh differs
- [ ] Yes / Maybe / No writes to Google and survives the next sync
- [ ] A recurring event offers This one / All of them, and This one leaves the series intact
- [ ] An RSVP never alters another attendee's response
- [ ] The patch carries `sendUpdates=all`, so the organiser is actually told
- [ ] A successful RSVP updates `self_response` locally, so the grid restyles at once
- [ ] A description containing HTML renders as text, and no `{@html}` exists in the UI
- [ ] RSVP is absent on read-only calendars and where the user is not a guest
- [ ] The popover works offline, showing last-synced state
- [ ] `cargo test --workspace` ≥ 205, `npm --prefix ui run test:ui` ≥ 145, clippy and `check` clean

## Deliberately not in this plan

- Creating, editing and deleting events; inviting people. Their own spec.
- Notifications and reminders — `reminders_json` stays unpopulated.
- Proposing a new time, or adding a note to a response.
- An account-removal path (a known limitation carried from Plan 1c).
