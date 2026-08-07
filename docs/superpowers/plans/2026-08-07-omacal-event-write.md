# Creating, Editing and Deleting Events — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let omacal create, edit and delete events — including recurring ones at three scopes — writing through to Google Calendar.

**Architecture:** Every write follows the path `respond_to_event` already proves: resolve the target Google id, build a patch body of *only* changed fields, write with `If-Match`, retry once on 412, then write the response back to the local database. The local DB stays a pure cache; there is no queue. Occurrence resolution reuses `Target` / `instance_lookup_window` / `resolve_instance_id`, which Plan 2 built and documented for exactly this.

**Tech Stack:** Tauri v2, Rust, sqlx + SQLite, Svelte 5 runes, wiremock, Playwright.

**Spec:** `docs/superpowers/specs/2026-08-07-omacal-event-write-design.md`

**Base:** `main` @ `988ae48` — 269 Rust tests, 316 UI tests.

## Global Constraints

- **Never** modify `~/Library/Application Support/com.omacal.app/omacal.db` or `~/.config/omacal/config.toml`. To inspect the DB, copy it to a temp dir — **and its `-wal` and `-shm` sidecars**, three files — then delete the copies.
- **Never** run write code against real credentials. It writes to a real calendar.
- **Never** `{:?}`-log, print or interpolate a `Tokens` value or any token string. `Tokens` has a hand-written redacting `Debug`; do not replace it with a derive.
- Use `sqlx::query` / `query_as` / `query_scalar`. **Never** the `query!` macros — they need `DATABASE_URL` at compile time.
- Svelte 5 runes only: `$props()`, `$state()`, `$derived()`, `$effect()`, `$bindable()`. No `export let`, no `$:`.
- **Never** render event text with `{@html}`. Descriptions are attacker-controlled.
- Colour comes from the Omarchy theme (`--accent`, `--bg`, `--hairline`, `--hour-rule`, `--muted`, `--surface`, `--text`, `--today-tint`). No hardcoded hex in app source.
- `selected` means *displayed*. `sync_enabled` means *fetched*. Never use one for the other.
- **No live network calls to Google in tests** — wiremock (Rust), harness stubs (UI).
- Attendees live in a **JSON column** on `events` (migration 0003), not a table.
- Playwright specs that depend on "now" must call `page.clock.setFixedTime`.
- Every test must be shown to fail against deliberately broken code, and **the mutation must be asserted present in the file before the suite is run**.

## File Structure

| File | Responsibility |
| --- | --- |
| `crates/omacal-google/src/client.rs` | + `insert_event`, `delete_event` |
| `src-tauri/src/write.rs` *(new)* | Pure builders: changed-field diffing, repeat↔RRULE, `UNTIL` truncation |
| `src-tauri/src/events.rs` | + `can_edit`, extended `EventDetail`, the three new commands |
| `src-tauri/src/lib.rs` | Register the new commands |
| `ui/src/lib/eventdetail.ts` | + types and bindings for the three commands |
| `ui/src/lib/EventForm.svelte` *(new)* | The create/edit form |
| `ui/src/lib/EventPopover.svelte` | + Edit and Delete entry points |
| `ui/src/App.svelte` | `n` key, click-empty-space, form state |

---

### Task 1: Google client — insert and delete

**Files:**
- Modify: `crates/omacal-google/src/client.rs` (after `event_instances`, ends line 244)

**Interfaces:**
- Produces: `insert_event(&self, cal: &str, body: &serde_json::Value) -> Result<model::Event, ApiError>`; `delete_event(&self, cal: &str, event_id: &str, etag: Option<&str>) -> Result<(), ApiError>`

- [ ] **Step 1: Write the failing tests**

Add to the existing `#[cfg(test)] mod tests` in `client.rs`:

```rust
#[tokio::test]
async fn insert_posts_the_body_and_never_notifies() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/calendars/cal%40x.com/events"))
        .and(query_param("sendUpdates", "none"))
        .and(body_json_string(r#"{"summary":"Lunch"}"#))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "new1", "status": "confirmed", "etag": "\"e1\""
        })))
        .mount(&server)
        .await;

    let c = CalendarClient::new(server.uri(), "tok");
    let ev = c
        .insert_event("cal@x.com", &serde_json::json!({"summary": "Lunch"}))
        .await
        .unwrap();
    assert_eq!(ev.id, "new1");
}

#[tokio::test]
async fn delete_sends_if_match_and_notifies_guests() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/calendars/c/events/e1"))
        .and(query_param("sendUpdates", "all"))
        .and(header("If-Match", "\"etag1\""))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    let c = CalendarClient::new(server.uri(), "tok");
    c.delete_event("c", "e1", Some("\"etag1\"")).await.unwrap();
}

/// Already gone is the caller's desired end state, not an error.
#[tokio::test]
async fn delete_treats_404_as_success() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let c = CalendarClient::new(server.uri(), "tok");
    c.delete_event("c", "gone", None).await.unwrap();
}

#[tokio::test]
async fn delete_surfaces_a_conflict_as_precondition_failed() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .respond_with(ResponseTemplate::new(412))
        .mount(&server)
        .await;

    let c = CalendarClient::new(server.uri(), "tok");
    assert!(matches!(
        c.delete_event("c", "e1", Some("\"old\"")).await,
        Err(ApiError::PreconditionFailed)
    ));
}
```

Add `query_param`, `header` and `body_json_string` to the existing `use wiremock::matchers::{...}` line.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p omacal-google insert_posts delete_sends delete_treats delete_surfaces`
Expected: FAIL — `no method named insert_event` / `delete_event`.

- [ ] **Step 3: Implement**

```rust
    /// Create an event. `sendUpdates=none` because a newly created event has
    /// no attendees to notify — omacal cannot add guests, so there is never
    /// anybody on a fresh event to tell.
    pub async fn insert_event(
        &self,
        cal: &str,
        body: &serde_json::Value,
    ) -> Result<model::Event, ApiError> {
        let resp = self
            .http
            .post(format!(
                "{}/calendars/{}/events",
                self.base_url,
                urlencoding_path(cal)
            ))
            .bearer_auth(&self.access_token)
            .query(&[("sendUpdates", "none")])
            .json(body)
            .send()
            .await
            .map_err(|e| ApiError::Transport(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(ApiError::Http(format!("{}", resp.status())));
        }

        resp.json::<model::Event>()
            .await
            .map_err(|e| ApiError::Transport(e.to_string()))
    }

    /// Delete an event. `sendUpdates=all` so a cancelled meeting reaches the
    /// guest list — a meeting that vanishes for the organiser only is worse
    /// than an email.
    ///
    /// `404` is success: the event is already gone, which is what the caller
    /// asked for. Returning an error there would make a double-click, or a
    /// retry after a dropped response, look like a failure.
    pub async fn delete_event(
        &self,
        cal: &str,
        event_id: &str,
        etag: Option<&str>,
    ) -> Result<(), ApiError> {
        let mut req = self
            .http
            .delete(format!(
                "{}/calendars/{}/events/{}",
                self.base_url,
                urlencoding_path(cal),
                urlencoding_path(event_id)
            ))
            .bearer_auth(&self.access_token)
            .query(&[("sendUpdates", "all")]);
        if let Some(etag) = etag {
            req = req.header("If-Match", etag);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| ApiError::Transport(e.to_string()))?;

        if resp.status() == reqwest::StatusCode::PRECONDITION_FAILED {
            return Err(ApiError::PreconditionFailed);
        }
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(());
        }
        if !resp.status().is_success() {
            return Err(ApiError::Http(format!("{}", resp.status())));
        }
        Ok(())
    }
```

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p omacal-google`
Expected: PASS.

- [ ] **Step 5: Mutation-check**

Change `sendUpdates` on delete from `"all"` to `"none"`. Confirm the mutation is in the file (`grep -n 'sendUpdates' crates/omacal-google/src/client.rs`), run `cargo test -p omacal-google`, confirm `delete_sends_if_match_and_notifies_guests` FAILS. Revert.

- [ ] **Step 6: Commit**

```bash
git add crates/omacal-google/src/client.rs
git commit -m "feat(google): insert and delete event methods"
```

---

### Task 2: `EventDetail` gains what editing needs

**Files:**
- Modify: `src-tauri/src/events.rs` (`EventDetail` at line 5, `event_detail_impl` at line 71)
- Modify: `ui/src/lib/eventdetail.ts` (`EventDetail` at line 11)

**Interfaces:**
- Consumes: `omacal_store::event_by_id(pool, id) -> Option<(StoredEvent, String)>` (the `String` is `access_role`)
- Produces: `EventDetail` gains `calendar_id: i64`, `recurrence: Option<String>`, `can_edit: bool`; `pub(crate) fn can_edit(demo: bool, access_role: &str) -> bool`

- [ ] **Step 1: Write the failing tests**

In `events.rs` tests:

```rust
#[test]
fn only_writable_calendars_are_editable() {
    assert!(can_edit(false, "owner"));
    assert!(can_edit(false, "writer"));
    assert!(!can_edit(false, "reader"));
    assert!(!can_edit(false, "freeBusyReader"));
}

/// Demo mode may not write, exactly as `can_respond` refuses it — the demo
/// calendars are seeded `owner`, so without this the form would offer a Save
/// that the write guard can only refuse.
#[test]
fn demo_mode_is_never_editable() {
    assert!(!can_edit(true, "owner"));
    assert!(!can_edit(true, "writer"));
}
```

And a DB-backed test proving the detail carries the raw rule through:

```rust
/// The Repeat control needs the real RRULE to decide whether it can represent
/// it (see `write::repeat_from_rrule`). Dropping it here would make every
/// exotic rule look like "Never" and invite a silent overwrite.
#[tokio::test]
async fn detail_carries_the_raw_recurrence_rule() {
    let pool = omacal_store::connect_memory().await.unwrap();
    let id = seed_one_event(&pool, |ev| {
        ev.recurrence = Some("RRULE:FREQ=MONTHLY;BYDAY=-1FR".into());
    })
    .await;

    let d = event_detail_impl(&state_with(pool, false), id).await.unwrap();
    assert_eq!(d.recurrence.as_deref(), Some("RRULE:FREQ=MONTHLY;BYDAY=-1FR"));
    assert!(d.can_edit);
}
```

> **Implementer:** `seed_one_event` and `state_with` — use whatever the existing
> `events.rs` tests already use for these; do **not** invent new helpers. Read
> the existing test module first and follow its established pattern. If no
> seeding helper exists, write the insert inline the way the neighbouring tests do.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p omacal only_writable demo_mode_is_never detail_carries`
Expected: FAIL — `cannot find function can_edit`.

- [ ] **Step 3: Implement**

```rust
/// Whether the edit and delete controls are shown at all.
///
/// Deliberately *not* `can_respond` minus its attendee clause: responding
/// needs a `self` attendee row to change, editing does not — you can edit an
/// event nobody else is on. Sharing an implementation would couple two rules
/// that only look alike.
pub(crate) fn can_edit(demo: bool, access_role: &str) -> bool {
    !demo && matches!(access_role, "owner" | "writer")
}
```

Add to `EventDetail`:

```rust
    pub calendar_id: i64,
    /// The raw `RRULE`, carried through unchanged so the UI can tell a rule it
    /// can represent from one it cannot.
    pub recurrence: Option<String>,
    pub can_edit: bool,
```

And in `event_detail_impl`, alongside the existing fields:

```rust
        calendar_id: event.calendar_id,
        recurrence: event.recurrence.clone(),
        can_edit: can_edit(state.demo, &access_role),
```

Note `event.recurrence` is moved into `is_recurring` by reference already; add `.clone()` as shown and keep the existing `is_recurring(&event.recurrence, &event.recurring_event_id)` call **before** the struct literal.

Mirror in `ui/src/lib/eventdetail.ts`:

```ts
  calendar_id: number;
  recurrence: string | null;
  can_edit: boolean;
```

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p omacal && npm --prefix ui run check`
Expected: PASS, 0 errors 0 warnings.

- [ ] **Step 5: Mutation-check**

Change `can_edit` to `!demo` (dropping the role check). Assert present, run, confirm `only_writable_calendars_are_editable` FAILS. Revert.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/events.rs ui/src/lib/eventdetail.ts
git commit -m "feat(events): detail carries calendar, raw rule and editability"
```

---

### Task 3: The changed-fields builder

This is spec §6 and the single most important safety property after occurrence identity. Pure function, no I/O.

**Files:**
- Create: `src-tauri/src/write.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod write;` beside the existing module declarations)

**Interfaces:**
- Produces:
  ```rust
  pub(crate) struct EventFields { /* full definition in Step 3 */ }
  pub(crate) fn changed_fields(before: &EventFields, after: &EventFields) -> serde_json::Value
  pub(crate) fn event_time_json(ms: i64, is_all_day: bool, tz: &str) -> serde_json::Value
  ```
- `EventInput` and `fields_from_input` are defined in **Task 4**, not here —
  they call `rrule_for`, which does not exist yet. Write the struct and the
  mapping there; this task defines only `EventFields` and the two functions above.

**The wire type is not the internal type, deliberately.** `EventFields.recurrence`
is `Option<Option<String>>` — the three-state the builder needs — and that does
not cross the Tauri boundary legibly: JSON has one `null`, not two. The command
takes `EventInput` instead, whose `repeat: Option<String>` carries the
dropdown's own vocabulary (absent = untouched, `Some("never")` = clear,
`Some("weekly")` = set), and `fields_from_input` maps it with
`repeat.map(|r| rrule_for(&r))` — which lands on `Some(None)` for `"never"`
exactly as intended, because `rrule_for("never")` is `None`.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> EventFields {
        EventFields {
            summary: Some("Standup".into()),
            location: None,
            description: None,
            start_ms: 1_785_398_400_000,
            end_ms: 1_785_400_200_000,
            is_all_day: false,
            tz: "Europe/Sofia".into(),
            recurrence: None,
        }
    }

    /// The property the whole module exists for. A fortnightly meeting whose
    /// title changes must not carry `recurrence` in its body — the Repeat
    /// dropdown cannot express "every 2nd Tuesday", so sending it would
    /// silently rewrite the real rule to something simpler.
    #[test]
    fn an_untouched_recurrence_is_never_sent() {
        let mut after = base();
        after.summary = Some("Standup (moved)".into());
        let body = changed_fields(&base(), &after);
        assert_eq!(body["summary"], "Standup (moved)");
        assert!(body.get("recurrence").is_none(), "body was {body}");
    }

    #[test]
    fn nothing_changed_produces_an_empty_body() {
        assert_eq!(changed_fields(&base(), &base()), serde_json::json!({}));
    }

    /// Clearing a field must send explicit null, not omit it — omitting means
    /// "leave alone" to a PATCH, so a cleared location would silently persist.
    #[test]
    fn clearing_a_field_sends_null_rather_than_omitting_it() {
        let mut before = base();
        before.location = Some("Room 4A".into());
        let body = changed_fields(&before, &base());
        assert!(body.get("location").is_some(), "body was {body}");
        assert!(body["location"].is_null());
    }

    /// Google rejects a body with start but not end when only one moved, and
    /// a half-moved event is meaningless anyway.
    #[test]
    fn moving_either_end_sends_both_times() {
        let mut after = base();
        after.end_ms += 900_000;
        let body = changed_fields(&base(), &after);
        assert!(body.get("start").is_some(), "body was {body}");
        assert!(body.get("end").is_some(), "body was {body}");
    }

    #[test]
    fn a_touched_repeat_is_sent_as_an_array() {
        let mut after = base();
        after.recurrence = Some(Some("RRULE:FREQ=WEEKLY".into()));
        let body = changed_fields(&base(), &after);
        assert_eq!(body["recurrence"], serde_json::json!(["RRULE:FREQ=WEEKLY"]));
    }

    /// Turning repetition off is `recurrence: null`, which Google reads as
    /// "make this a single event".
    #[test]
    fn repeat_set_to_never_sends_null() {
        let mut after = base();
        after.recurrence = Some(None);
        let body = changed_fields(&base(), &after);
        assert!(body["recurrence"].is_null());
    }

    #[test]
    fn a_timed_event_sends_datetime_and_zone() {
        let v = event_time_json(1_785_398_400_000, false, "Europe/Sofia");
        assert!(v["dateTime"].is_string());
        assert_eq!(v["timeZone"], "Europe/Sofia");
        assert!(v.get("date").is_none());
    }

    /// All-day events use `date`, never `dateTime` — Google rejects the mix.
    #[test]
    fn an_all_day_event_sends_a_bare_date() {
        let v = event_time_json(1_785_398_400_000, true, "Europe/Sofia");
        assert!(v["date"].is_string());
        assert_eq!(v["date"].as_str().unwrap().len(), 10);
        assert!(v.get("dateTime").is_none());
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p omacal write::`
Expected: FAIL — module does not exist.

- [ ] **Step 3: Implement**

Create `src-tauri/src/write.rs`. Use `omacal_sync::to_rfc3339` for the timed case — **do not write a second formatter**. For the all-day case, format the civil date in `tz` with `jiff`.

```rust
//! Pure builders for event write bodies.
//!
//! Everything here is a function of its arguments: no pool, no client, no
//! clock. The write commands stay thin wrappers around these so the rules
//! that matter — "never send a field the user did not touch", "all-day means
//! `date` not `dateTime`" — are testable without a server.

use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EventFields {
    pub summary: Option<String>,
    pub location: Option<String>,
    pub description: Option<String>,
    pub start_ms: i64,
    pub end_ms: i64,
    pub is_all_day: bool,
    /// IANA zone the times are authored in.
    pub tz: String,
    /// Three-state, and the distinction is the point:
    /// `None` — the user did not touch Repeat; omit `recurrence` entirely.
    /// `Some(None)` — the user chose Never; send `null`.
    /// `Some(Some(rule))` — send `[rule]`.
    pub recurrence: Option<Option<String>>,
}

/// What the UI actually sends. Distinct from [`EventFields`] because the
/// three-state above needs two levels of `Option` and JSON has one `null`.
///
/// `repeat` carries the dropdown's own vocabulary rather than an RRULE: the UI
/// has no business authoring iCalendar, and keeping the mapping in one place
/// ([`rrule_for`]) is what makes "a rule we cannot express is never
/// overwritten" checkable.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EventInput {
    pub summary: Option<String>,
    pub location: Option<String>,
    pub description: Option<String>,
    pub start_ms: i64,
    pub end_ms: i64,
    pub is_all_day: bool,
    pub tz: String,
    /// Absent when the user did not touch Repeat.
    #[serde(default)]
    pub repeat: Option<String>,
}

/// `"never"` maps to `Some(None)` — clear the rule — because [`rrule_for`]
/// returns `None` for it. That is the one case worth staring at: an absent
/// `repeat` and a `repeat` of `"never"` must not collapse together.
pub(crate) fn fields_from_input(input: EventInput) -> EventFields {
    EventFields {
        summary: input.summary,
        location: input.location,
        description: input.description,
        start_ms: input.start_ms,
        end_ms: input.end_ms,
        is_all_day: input.is_all_day,
        tz: input.tz,
        recurrence: input.repeat.map(|r| rrule_for(&r)),
    }
}

/// Google's `start`/`end` object. All-day events carry `date`; timed events
/// carry `dateTime` and `timeZone`. Sending both is rejected.
pub(crate) fn event_time_json(ms: i64, is_all_day: bool, tz: &str) -> Value {
    if is_all_day {
        let zoned = jiff::Timestamp::from_millisecond(ms)
            .unwrap_or(jiff::Timestamp::UNIX_EPOCH)
            .in_tz(tz)
            .unwrap_or_else(|_| {
                jiff::Timestamp::from_millisecond(ms)
                    .unwrap_or(jiff::Timestamp::UNIX_EPOCH)
                    .in_tz("UTC")
                    .expect("UTC always resolves")
            });
        json!({ "date": zoned.date().to_string() })
    } else {
        json!({ "dateTime": omacal_sync::to_rfc3339(ms), "timeZone": tz })
    }
}

/// A PATCH body carrying only what actually changed.
///
/// A field absent from a PATCH body means "leave it alone"; a field present
/// and null means "clear it". Both are needed, and conflating them makes
/// clearing a location impossible.
pub(crate) fn changed_fields(before: &EventFields, after: &EventFields) -> Value {
    let mut body = serde_json::Map::new();

    let mut text = |key: &str, b: &Option<String>, a: &Option<String>| {
        if b != a {
            body.insert(
                key.to_string(),
                match a {
                    Some(s) => Value::String(s.clone()),
                    None => Value::Null,
                },
            );
        }
    };
    text("summary", &before.summary, &after.summary);
    text("location", &before.location, &after.location);
    text("description", &before.description, &after.description);

    // Times move as a pair. Google rejects a body that redefines one end of
    // an event without the other when the all-day flag is in play, and half a
    // move is not a thing a user can mean.
    if before.start_ms != after.start_ms
        || before.end_ms != after.end_ms
        || before.is_all_day != after.is_all_day
    {
        body.insert(
            "start".into(),
            event_time_json(after.start_ms, after.is_all_day, &after.tz),
        );
        body.insert(
            "end".into(),
            event_time_json(after.end_ms, after.is_all_day, &after.tz),
        );
    }

    match &after.recurrence {
        None => {}
        Some(None) => {
            body.insert("recurrence".into(), Value::Null);
        }
        Some(Some(rule)) => {
            body.insert("recurrence".into(), json!([rule]));
        }
    }

    Value::Object(body)
}
```

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p omacal write::`
Expected: PASS, 8 tests.

- [ ] **Step 5: Mutation-check the field that matters**

Replace the `match &after.recurrence` block with an unconditional
`body.insert("recurrence".into(), json!([after.recurrence.clone().flatten()]));`.
Assert the mutation is present, run, confirm `an_untouched_recurrence_is_never_sent` FAILS. Revert.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/write.rs src-tauri/src/lib.rs
git commit -m "feat(write): build patch bodies from only what changed"
```

---

### Task 4: Repeat ↔ RRULE

**Files:**
- Modify: `src-tauri/src/write.rs`

**Interfaces:**
- Produces: `pub(crate) fn rrule_for(repeat: &str) -> Option<String>`; `pub(crate) fn repeat_from_rrule(rule: Option<&str>) -> String`; plus `EventInput` and `fields_from_input` (deferred here from Task 3 because they call `rrule_for`)

The six values the UI offers: `never`, `daily`, `weekdays`, `weekly`, `monthly`, `yearly`. Anything else round-trips as `custom`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn each_offered_repeat_maps_to_a_rule() {
    assert_eq!(rrule_for("never"), None);
    assert_eq!(rrule_for("daily").as_deref(), Some("RRULE:FREQ=DAILY"));
    assert_eq!(
        rrule_for("weekdays").as_deref(),
        Some("RRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR")
    );
    assert_eq!(rrule_for("weekly").as_deref(), Some("RRULE:FREQ=WEEKLY"));
    assert_eq!(rrule_for("monthly").as_deref(), Some("RRULE:FREQ=MONTHLY"));
    assert_eq!(rrule_for("yearly").as_deref(), Some("RRULE:FREQ=YEARLY"));
}

#[test]
fn every_rule_we_author_reads_back_as_itself() {
    for r in ["daily", "weekdays", "weekly", "monthly", "yearly"] {
        let rule = rrule_for(r).unwrap();
        assert_eq!(repeat_from_rrule(Some(&rule)), r, "round trip failed for {r}");
    }
    assert_eq!(repeat_from_rrule(None), "never");
}

/// The property that stops a silent overwrite: a rule the dropdown cannot
/// express must be reported as `custom`, so the UI can disable the control
/// rather than offering to replace it with something simpler.
#[test]
fn a_rule_we_cannot_express_is_custom() {
    for exotic in [
        "RRULE:FREQ=MONTHLY;BYDAY=-1FR",
        "RRULE:FREQ=WEEKLY;INTERVAL=2",
        "RRULE:FREQ=DAILY;COUNT=5",
        "RRULE:FREQ=WEEKLY;BYDAY=MO,WE",
        "RRULE:FREQ=WEEKLY;UNTIL=20261231T000000Z",
    ] {
        assert_eq!(repeat_from_rrule(Some(exotic)), "custom", "{exotic}");
    }
}

/// The two states JSON cannot tell apart on its own, and the reason
/// `EventInput` exists. An absent `repeat` must leave the rule alone; an
/// explicit `"never"` must clear it. Collapsing them makes every title edit
/// on a recurring event either impossible or destructive.
#[test]
fn an_absent_repeat_and_an_explicit_never_are_different_things() {
    let mut input = sample_input();
    input.repeat = None;
    assert_eq!(fields_from_input(input).recurrence, None);

    let mut input = sample_input();
    input.repeat = Some("never".into());
    assert_eq!(fields_from_input(input).recurrence, Some(None));

    let mut input = sample_input();
    input.repeat = Some("weekly".into());
    assert_eq!(
        fields_from_input(input).recurrence,
        Some(Some("RRULE:FREQ=WEEKLY".into()))
    );
}
```

> **Implementer:** write `sample_input()` in the test module alongside the
> existing `base()` helper.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p omacal write::`
Expected: FAIL — functions not found.

- [ ] **Step 3: Implement**

```rust
/// The rule omacal writes for each Repeat option. `never` is `None`.
pub(crate) fn rrule_for(repeat: &str) -> Option<String> {
    Some(
        match repeat {
            "daily" => "RRULE:FREQ=DAILY",
            "weekdays" => "RRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR",
            "weekly" => "RRULE:FREQ=WEEKLY",
            "monthly" => "RRULE:FREQ=MONTHLY",
            "yearly" => "RRULE:FREQ=YEARLY",
            _ => return None,
        }
        .to_string(),
    )
}

/// Which Repeat option, if any, represents `rule` exactly.
///
/// Exact string equality against what [`rrule_for`] authors, deliberately —
/// not a parse. A rule carrying `INTERVAL`, `COUNT`, `UNTIL`, `EXDATE` or a
/// `BYDAY` we did not write is `custom`, and the UI must then refuse to
/// overwrite it. Being generous here (parsing `FREQ` and ignoring the rest)
/// is exactly how "every 2nd Tuesday" becomes "weekly" behind the user's back.
pub(crate) fn repeat_from_rrule(rule: Option<&str>) -> String {
    let Some(rule) = rule else {
        return "never".into();
    };
    for candidate in ["daily", "weekdays", "weekly", "monthly", "yearly"] {
        if rrule_for(candidate).as_deref() == Some(rule) {
            return candidate.into();
        }
    }
    "custom".into()
}
```

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p omacal write::`
Expected: PASS.

- [ ] **Step 5: Mutation-check**

Make `repeat_from_rrule` match on a `FREQ=` prefix instead of full equality
(e.g. `if rule.starts_with("RRULE:FREQ=WEEKLY") { return "weekly".into() }`).
Assert present, run, confirm `a_rule_we_cannot_express_is_custom` FAILS on the
`INTERVAL=2` case. Revert.

Then change `fields_from_input` to `recurrence: Some(input.repeat.and_then(|r| rrule_for(&r)))`
— the plausible simplification that collapses "untouched" into "clear it".
Assert present, run, confirm `an_absent_repeat_and_an_explicit_never_are_different_things`
FAILS. Revert.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/write.rs
git commit -m "feat(write): map Repeat options to and from RRULE, exactly"
```

---

### Task 5: The create command

**Files:**
- Modify: `src-tauri/src/events.rs`
- Modify: `src-tauri/src/lib.rs` (register `events::create_event`)
- Modify: `ui/src/lib/eventdetail.ts`

**Interfaces:**
- Consumes: `write::{EventFields, changed_fields, event_time_json, rrule_for}`, `omacal_google::CalendarClient::insert_event`, `omacal_store::upsert_event`
- Produces: `create_event(state, calendar_id, fields) -> Result<EventDetail, String>`, split as `create_impl` / `create_via_client` exactly as `respond_impl` / `respond_via_client` are, so tests can inject a wiremock-backed client

- [ ] **Step 1: Write the failing tests**

```rust
/// Demo mode must reach neither Google nor the real database. Same guard
/// shape as `respond`, and asserted the same way: the demo failure must be
/// the demo message, not a config or keyring error.
#[tokio::test]
async fn creating_refuses_in_demo_mode_without_touching_config_keyring_or_google() {
    let pool = omacal_store::connect_memory().await.unwrap();
    let err = create_impl(&state_with(pool, true), 1, sample_fields())
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("demo"), "got: {err}");
}

/// A subscribed holiday calendar is `reader`. Creating into it must be
/// refused before any request is built, not left to Google's 403.
#[tokio::test]
async fn creating_into_a_read_only_calendar_is_refused() {
    let pool = omacal_store::connect_memory().await.unwrap();
    let cal = seed_calendar(&pool, "reader").await;
    let err = create_impl(&state_with(pool, false), cal, sample_fields())
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("not writable"), "got: {err}");
}

#[tokio::test]
async fn a_created_event_is_stored_locally() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "g-new", "status": "confirmed", "etag": "\"e1\"",
            "summary": "Lunch",
            "start": {"dateTime": "2026-08-10T12:00:00+03:00"},
            "end":   {"dateTime": "2026-08-10T13:00:00+03:00"}
        })))
        .mount(&server)
        .await;

    let pool = omacal_store::connect_memory().await.unwrap();
    let cal = seed_calendar(&pool, "owner").await;
    let client = omacal_google::CalendarClient::new(server.uri(), "tok");

    let id = create_via_client(&pool, cal, "cal@x.com", sample_fields(), &client)
        .await
        .unwrap();

    let (row, _) = omacal_store::event_by_id(&pool, id).await.unwrap().unwrap();
    assert_eq!(row.google_id, "g-new");
}
```

> **Implementer:** `seed_calendar` and `sample_fields` are yours to write in the
> test module if nothing equivalent exists. Check first. `state_with` already exists.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p omacal creating_ a_created_event`
Expected: FAIL — functions not found.

- [ ] **Step 3: Implement**

Follow `respond_impl` / `respond_via_client` (lines 290 and 334) as the template — the split exists so a test can inject a client without touching `load_config` or the keyring. `create_impl` resolves the calendar's `google_id`, `access_role` and account email, applies the demo guard first and the writability check second, then delegates.

The insert body is built from `EventFields` directly (not `changed_fields` — everything is new):

```rust
    let mut body = serde_json::json!({
        "start": crate::write::event_time_json(f.start_ms, f.is_all_day, &f.tz),
        "end":   crate::write::event_time_json(f.end_ms,   f.is_all_day, &f.tz),
    });
    if let Some(s) = &f.summary     { body["summary"]     = s.clone().into(); }
    if let Some(s) = &f.location    { body["location"]    = s.clone().into(); }
    if let Some(s) = &f.description { body["description"] = s.clone().into(); }
    if let Some(Some(rule)) = &f.recurrence {
        body["recurrence"] = serde_json::json!([rule]);
    }
```

Store the returned event with `omacal_store::upsert_event`, mapping Google's
response the same way sync does. **Reuse `omacal_sync`'s existing conversion
rather than writing a second one** — find it before writing anything.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p omacal`
Expected: PASS.

- [ ] **Step 5: Mutation-check**

Remove the writability check. Assert present, run, confirm
`creating_into_a_read_only_calendar_is_refused` FAILS. Revert. Then remove the
demo guard and confirm the demo test FAILS. Revert.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/events.rs src-tauri/src/lib.rs ui/src/lib/eventdetail.ts
git commit -m "feat(events): create_event command"
```

---

### Task 6: The edit command — this occurrence and all events

**Files:**
- Modify: `src-tauri/src/events.rs`
- Modify: `src-tauri/src/lib.rs`, `ui/src/lib/eventdetail.ts`

**Interfaces:**
- Consumes: `Target`, `target_event_id`, `instance_lookup_window`, `resolve_instance_id`, `merge_patched` (all `pub(crate)` in `events.rs`), `write::changed_fields`
- Produces: `update_event(state, id, scope, occurrence_start_ms, fields) -> Result<EventDetail, String>` with `update_impl` / `update_via_client`

`scope` is `"this"` or `"all"` here. `"following"` is Task 7.

The occurrence-resolution code is **already correct and already documented** — read the doc comments on `instance_lookup_window` (line 194) and `resolve_instance_id` (line 220) before writing anything, and call them rather than re-deriving. `respond_via_client` line 334 shows the exact call sequence including the `recurring_event_id.or(own_id)` handling for a bare master.

- [ ] **Step 1: Write the failing tests**

```rust
/// The defect this whole design guards against. "This one" must patch the
/// instance id Google returns, never the master's — a master patch with
/// sendUpdates=all rewrites every occurrence and tells the whole guest list.
#[tokio::test]
async fn editing_one_occurrence_patches_the_instance_not_the_master() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/calendars/cal%40x.com/events/master1/instances"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [{"id": "master1_20260810T090000Z", "status": "confirmed",
                       "etag": "\"i1\""}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/calendars/cal%40x.com/events/master1_20260810T090000Z"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "master1_20260810T090000Z", "status": "confirmed", "etag": "\"i2\""
        })))
        .expect(1)
        .mount(&server)
        .await;
    // No PATCH on the master is mounted: one arriving there is a 404 and the
    // test fails on the unwrap below rather than passing quietly.

    // ... drive update_via_client with scope "this" ...
}

/// An occurrence that resolves to nothing must fail loudly. Plan 2's original
/// fallback silently widened "this one" into "all of them".
#[tokio::test]
async fn an_unresolvable_occurrence_is_an_error_not_a_master_patch() {
    // instances returns {"items": []} for a bare master; assert Err, and
    // assert no PATCH was received.
}

#[tokio::test]
async fn editing_all_events_patches_the_master() {
    // scope "all" — PATCH lands on master1, and `instances` is never called.
}

/// Spec §6 end to end, not just in the pure builder.
#[tokio::test]
async fn editing_a_title_never_sends_recurrence() {
    // Seed a master with RRULE:FREQ=MONTHLY;BYDAY=-1FR, change only the title,
    // assert the received PATCH body has no `recurrence` key.
}
```

> **Implementer:** fill in the driving code following the existing
> `respond_via_client` tests in this file — they already mount `instances` +
> `PATCH` pairs against wiremock and assert on the received body. Match their
> shape rather than inventing a new harness.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p omacal editing_ an_unresolvable`
Expected: FAIL.

- [ ] **Step 3: Implement**

`update_via_client`:
1. `let series = ev.recurring_event_id.as_deref().or_else(|| ev.recurrence.as_ref().map(|_| ev.google_id.as_str()));`
2. `match target_event_id(scope, series, &ev.google_id)`
3. `Target::Instance { master, fallback }` → `event_instances` over `instance_lookup_window(occurrence_start_ms)` → `resolve_instance_id`
4. `changed_fields(&before, &after)`; if the body is empty, return early without a request
5. `patch_event(cal, &target_id, &body, ev.etag.as_deref())`
6. On `ApiError::PreconditionFailed`: `get_event`, rebuild `before` from the fresh copy, rebuild the body, retry **once**
7. `merge_patched(&mut row, &patched)` and `upsert_event`

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p omacal`

- [ ] **Step 5: Mutation-check**

Change step 3 to use `master` directly instead of the resolved instance id.
Assert present, run, confirm `editing_one_occurrence_patches_the_instance_not_the_master` FAILS. Revert.

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat(events): update_event for this-occurrence and all-events"
```

---

### Task 7: "This and following"

The riskiest task in the plan. Its own task so it gets its own reviewer.

**Files:**
- Modify: `src-tauri/src/write.rs` (the `UNTIL` builder), `src-tauri/src/events.rs`

**Interfaces:**
- Produces: `pub(crate) fn truncated_rule(rule: &str, before_ms: i64) -> String`

- [ ] **Step 1: Write the failing tests**

```rust
/// UNTIL is inclusive in RFC 5545, so it must land strictly before the
/// occurrence that moves to the new series — one second earlier. Getting this
/// wrong duplicates that occurrence in both series or drops it from both.
#[test]
fn until_lands_one_second_before_the_split() {
    let r = truncated_rule("RRULE:FREQ=WEEKLY", 1_785_398_400_000);
    assert_eq!(r, "RRULE:FREQ=WEEKLY;UNTIL=20260730T075959Z");
}

/// An existing UNTIL is replaced, not appended — two UNTILs is invalid.
#[test]
fn an_existing_until_is_replaced() {
    let r = truncated_rule("RRULE:FREQ=WEEKLY;UNTIL=20271231T000000Z", 1_785_398_400_000);
    assert_eq!(r.matches("UNTIL").count(), 1);
    assert!(r.ends_with("UNTIL=20260730T075959Z"));
}

/// COUNT and UNTIL are mutually exclusive in RFC 5545; a rule carrying COUNT
/// must lose it when truncated.
#[test]
fn count_is_dropped_when_until_is_added() {
    let r = truncated_rule("RRULE:FREQ=DAILY;COUNT=10", 1_785_398_400_000);
    assert!(!r.contains("COUNT"), "got {r}");
    assert!(r.contains("UNTIL="));
}
```

> **Implementer:** verify the expected `UNTIL` strings yourself before trusting
> them. Compute the UTC instant of `1_785_398_400_000 - 1000` and format it
> `%Y%m%dT%H%M%SZ`. If my values are wrong, **fix the plan's values, not the
> function** — and say so in your report. Plans in this project have shipped
> wrong epoch constants more than once.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p omacal until_ an_existing_until count_is_dropped`

- [ ] **Step 3: Implement `truncated_rule`, then the two-write sequence**

The sequence, and **the order is the safety property**:

1. **Create first.** `insert_event` with the remaining recurrence, `DTSTART` at this occurrence, copying summary/location/description/times from the (edited) fields.
2. **Truncate second.** `patch_event` on the master with `{"recurrence": [truncated_rule(...)]}`.

If step 2 fails, the error must name the leftover duplicate:

```rust
anyhow::bail!(
    "the new series was created but the original could not be shortened — \
     you now have two overlapping series and should delete one"
)
```

- [ ] **Step 4: Write the ordering test**

```rust
/// Order is the whole safety argument. Create-then-truncate leaves a visible,
/// deletable duplicate if the second write fails; truncate-then-create loses
/// the tail silently and unrecoverably.
#[tokio::test]
async fn following_creates_the_new_series_before_truncating_the_old() {
    // Mount POST and PATCH against one wiremock server, then assert on
    // `server.received_requests().await` that the POST index < the PATCH index.
}

#[tokio::test]
async fn a_failed_truncate_reports_the_leftover_duplicate() {
    // POST succeeds, PATCH returns 500. Assert Err whose message mentions
    // "two overlapping series", and that the local DB was not told the
    // operation succeeded.
}
```

- [ ] **Step 5: Run, then mutation-check**

Swap the two writes so truncate happens first. Assert present, run, confirm
`following_creates_the_new_series_before_truncating_the_old` FAILS. Revert.

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat(events): this-and-following, create before truncate"
```

---

### Task 8: The delete command

**Files:**
- Modify: `src-tauri/src/events.rs`, `src-tauri/src/lib.rs`, `ui/src/lib/eventdetail.ts`

**Interfaces:**
- Produces: `delete_event_cmd(state, id, scope, occurrence_start_ms) -> Result<(), String>` (named to avoid colliding with `omacal_store::delete_event`), with `delete_impl` / `delete_via_client`

Scopes: `this` → resolve instance, `delete_event` on it. `all` → `delete_event` on the master. `following` → `patch_event` on the master with `truncated_rule` (no insert; there is no tail to keep).

- [ ] **Step 1: Write the failing tests**

```rust
#[tokio::test]
async fn deleting_one_occurrence_deletes_the_instance_not_the_master() { /* ... */ }

#[tokio::test]
async fn deleting_all_deletes_the_master() { /* ... */ }

/// "Following" is a truncation, not a delete — deleting the master would take
/// the past occurrences with it.
#[tokio::test]
async fn deleting_following_truncates_and_never_issues_a_delete() {
    // Assert a PATCH was received and NO DELETE was.
}

#[tokio::test]
async fn deleting_removes_the_local_row() { /* ... */ }

#[tokio::test]
async fn deleting_refuses_in_demo_mode() { /* ... */ }
```

- [ ] **Step 2–4: Run failing, implement, run passing**

Local removal uses the existing `omacal_store::delete_event`. For scope `this`
on a series the local master row stays — only Google knows the occurrence is
gone until the next sync, so trigger a refresh rather than deleting the row.

- [ ] **Step 5: Mutation-check**

Change the `following` arm to issue a `delete_event` on the master. Assert
present, run, confirm `deleting_following_truncates_and_never_issues_a_delete`
FAILS. Revert.

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat(events): delete_event command across three scopes"
```

---

### Task 9: The event form

**Files:**
- Create: `ui/src/lib/EventForm.svelte`
- Create: tests in `ui/tests/components.spec.ts`, fixtures in `ui/tests/fixtures.ts`

**Interfaces:**
- Consumes: `placePopover` from `ui/src/lib/position.ts` (pure geometry, takes an anchor rect — unchanged), `getCalendars` from `ui/src/lib/calendars.ts`
- Props: `{ anchor: DOMRect, initial: EventFormValue, calendars: CalendarRow[], onsave, oncancel }`

Fields: title, date, start, end, all-day toggle, location, description, calendar, repeat.

- [ ] **Step 1: Write the failing specs**

```ts
test('only writable calendars are offered', async ({ mount }) => {
  // calendars fixture: one owner, one writer, one reader.
  // Assert the select has exactly 2 options and none is the reader's name.
});

test('save is refused when the end is before the start', async ({ mount }) => {
  // Assert an inline message appears and onsave was NOT called.
});

test('an unrepresentable repeat rule is shown as a disabled Custom option', async ({ mount }) => {
  // initial.repeat = 'custom'. Assert the select is disabled and shows the
  // rule in words. This is the UI half of spec §6.
});

test('an event with guests warns that saving notifies them', async ({ mount }) => {
  // initial with 4 attendees. Assert the notice names the count.
});

test('a description is rendered as text, never as markup', async ({ mount }) => {
  // initial.description = '<img src=x onerror=alert(1)>'
  // Assert no <img> element exists and the raw text is visible.
});
```

Every spec must call `page.clock.setFixedTime` — the form defaults to "the next half hour".

- [ ] **Step 2: Run to verify they fail**

Run: `npm --prefix ui run test:ui -- --grep "writable calendars|end is before|Custom option|notifies them|never as markup"`

- [ ] **Step 3: Implement `EventForm.svelte`**

Svelte 5 runes only. Theme variables only — no hardcoded hex. Never `{@html}`.

- [ ] **Step 4: Run to verify they pass**

- [ ] **Step 5: Mutation-check**

Remove the `access_role` filter on the calendar list. Assert present, run,
confirm `only writable calendars are offered` FAILS. Revert.

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat(ui): the event form"
```

---

### Task 10: Wiring

**Files:**
- Modify: `ui/src/App.svelte`, `ui/src/lib/EventPopover.svelte`, `ui/src/lib/WeekGrid.svelte`
- Modify: `ui/tests/app.spec.ts`

Entry points: `n` anywhere; click on empty grid space; **Edit** and **Delete** in the popover, shown only when `detail.can_edit`.

The recurrence scope chooser appears **only** when `detail.is_recurring`, offering This / This and following / All. Delete confirms, naming the event and the scope, and says when guests will be told.

- [ ] **Step 1: Write the failing specs**

```ts
test('n opens the form on the anchor date', async ({ page }) => { /* ... */ });

test('clicking empty grid space opens the form at that time', async ({ page }) => { /* ... */ });

test('a non-recurring event offers no scope choice', async ({ page }) => { /* ... */ });

test('a recurring event offers all three scopes', async ({ page }) => { /* ... */ });

/// The occurrence-identity property, at the top of the stack: the clicked
/// block's own start_ms must reach the command, not detail.start_ms.
test('editing an occurrence sends the clicked block start, not the series start', async ({ page }) => {
  // Stub the command in the harness and assert on the argument it received.
});

test('delete asks for confirmation and names the event', async ({ page }) => { /* ... */ });

test('edit and delete are hidden when can_edit is false', async ({ page }) => { /* ... */ });
```

- [ ] **Step 2–4: Run failing, implement, run passing**

- [ ] **Step 5: Mutation-check**

Change the call site to pass `detail.start_ms` instead of the clicked block's
`start_ms` — the exact defect Plan 2 shipped. Assert present, run, confirm
`editing an occurrence sends the clicked block start` FAILS. Revert.

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat(ui): wire create, edit and delete into the views"
```

---

## Definition of Done

- [ ] `cargo test --workspace` ≥ 300 passed, 0 failed
- [ ] `npm --prefix ui run test:ui` ≥ 340 passed
- [ ] `npm --prefix ui run check` — 0 errors, 0 warnings
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` — clean
- [ ] Every mutation listed in Step 5 of each task has been shown to fail the named test, with the mutation asserted present first
- [ ] No live network call to Google anywhere in the suite
- [ ] Demo mode reaches neither Google nor the real database on create, edit or delete
- [ ] A `reader` calendar is offerable nowhere

> **On the test-count bars:** these are estimates. If the honest final number
> is below a bar, report the real number and say why — do **not** pad the suite
> to hit it, and do **not** silently lower the bar. Both have been tried on this
> project and both were caught.
