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
    /// Set on an *exception*: the `google_id` of the series this row overrides.
    /// The pair `(recurring_event_id, original_start_utc)` names exactly one
    /// occurrence of that series, which the renderer must then suppress — the
    /// master keeps expanding into the slot otherwise, and you get a ghost.
    pub recurring_event_id: Option<String>,
    /// The instant the overridden occurrence *would* have started at. Not the
    /// same as `start_utc` once the instance has been moved.
    pub original_start_utc: Option<i64>,
    pub status: String,
    pub self_response: Option<String>,
    pub conference_uri: Option<String>,
    /// The owning calendar's colour, joined in by `events_in_window`. It lives
    /// on `calendars`, not `events`, so `upsert_event` neither reads nor writes
    /// it; a hand-built `StoredEvent` on the write path leaves it `None`.
    pub color_hex: Option<String>,
    pub description: Option<String>,
    pub etag: Option<String>,
    pub sequence: i64,
    pub organizer_email: Option<String>,
    pub attendees: Vec<Attendee>,
}

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

const SELECT_COLS: &str = "e.id, e.calendar_id, e.google_id, e.summary, e.location,
     e.start_utc, e.end_utc, e.start_tz, e.end_tz, e.is_all_day, e.recurrence,
     e.recurring_event_id, e.original_start_utc,
     e.status, e.self_response, e.conference_uri, c.color_hex,
     e.description, e.etag, e.sequence, e.organizer_email, e.attendees_json";

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
        recurring_event_id: row.get("recurring_event_id"),
        original_start_utc: row.get("original_start_utc"),
        status: row.get("status"),
        self_response: row.get("self_response"),
        conference_uri: row.get("conference_uri"),
        color_hex: row.get("color_hex"),
        description: row.get("description"),
        etag: row.get("etag"),
        sequence: row.get("sequence"),
        organizer_email: row.get("organizer_email"),
        // A malformed or absent JSON column must not fail the whole window
        // query — most personal events have no guests at all, so `NULL` here
        // is the common path, not an edge case.
        attendees: row
            .get::<Option<String>, _>("attendees_json")
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default(),
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Generic over the executor rather than taking `&SqlitePool`, so the sync path
/// can run this inside the same transaction as its `sync_enabled` re-check. A
/// `&SqlitePool` still satisfies the bound, so every other call site is
/// unchanged.
pub async fn upsert_event<'e, E>(exec: E, ev: &StoredEvent) -> anyhow::Result<i64>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    // 21 columns, 21 placeholders, 21 binds, all in the same order. Keep them
    // that way: a mismatch here writes a value into the wrong column silently.
    let attendees_json = serde_json::to_string(&ev.attendees)?;
    let id: i64 = sqlx::query(
        "INSERT INTO events (calendar_id, google_id, summary, location, start_utc, end_utc,
             start_tz, end_tz, is_all_day, recurrence, recurring_event_id,
             original_start_utc, status, self_response, conference_uri, updated_at,
             description, etag, sequence, organizer_email, attendees_json)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21)
         ON CONFLICT (calendar_id, google_id) DO UPDATE SET
             summary = excluded.summary, location = excluded.location,
             start_utc = excluded.start_utc, end_utc = excluded.end_utc,
             start_tz = excluded.start_tz, end_tz = excluded.end_tz,
             is_all_day = excluded.is_all_day, recurrence = excluded.recurrence,
             recurring_event_id = excluded.recurring_event_id,
             original_start_utc = excluded.original_start_utc,
             status = excluded.status, self_response = excluded.self_response,
             conference_uri = excluded.conference_uri, updated_at = excluded.updated_at,
             description = excluded.description, etag = excluded.etag,
             sequence = excluded.sequence, organizer_email = excluded.organizer_email,
             attendees_json = excluded.attendees_json
         RETURNING id",
    )
    .bind(ev.calendar_id)          // ?1  calendar_id
    .bind(&ev.google_id)           // ?2  google_id
    .bind(&ev.summary)             // ?3  summary
    .bind(&ev.location)            // ?4  location
    .bind(ev.start_utc)            // ?5  start_utc
    .bind(ev.end_utc)              // ?6  end_utc
    .bind(&ev.start_tz)            // ?7  start_tz
    .bind(&ev.end_tz)              // ?8  end_tz
    .bind(ev.is_all_day as i64)    // ?9  is_all_day
    .bind(&ev.recurrence)          // ?10 recurrence
    .bind(&ev.recurring_event_id)  // ?11 recurring_event_id
    .bind(ev.original_start_utc)   // ?12 original_start_utc
    .bind(&ev.status)              // ?13 status
    .bind(&ev.self_response)       // ?14 self_response
    .bind(&ev.conference_uri)      // ?15 conference_uri
    .bind(now_ms())                // ?16 updated_at
    .bind(&ev.description)         // ?17 description
    .bind(&ev.etag)                // ?18 etag
    .bind(ev.sequence)             // ?19 sequence
    .bind(&ev.organizer_email)     // ?20 organizer_email
    .bind(attendees_json)          // ?21 attendees_json
    .fetch_one(exec)
    .await?
    .get("id");
    Ok(id)
}

/// Generic over the executor for the same reason as [`upsert_event`].
pub async fn delete_event<'e, E>(
    exec: E,
    calendar_id: i64,
    google_id: &str,
) -> anyhow::Result<()>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    sqlx::query("DELETE FROM events WHERE calendar_id = ?1 AND google_id = ?2")
        .bind(calendar_id)
        .bind(google_id)
        .execute(exec)
        .await?;
    Ok(())
}

/// The query behind both [`event_by_id`] and [`event_for_write`]: one event
/// row, its calendar's `access_role`, the calendar's own `google_id`, and the
/// email of the account that owns it. The last two need a second join beyond
/// `calendars` — they live on `accounts` — because an RSVP has to know which
/// account's access token to use, not just whether the calendar is writable.
///
/// `c.google_id` is aliased: `SELECT_COLS` already selects the *event's* own
/// `google_id` as `e.google_id`, and an unaliased second column of the same
/// name would collide when read back by name.
async fn event_row_for_write(
    pool: &SqlitePool,
    id: i64,
) -> anyhow::Result<Option<(StoredEvent, String, String, String)>> {
    let sql = format!(
        "SELECT {SELECT_COLS}, c.access_role, c.google_id AS cal_google_id, a.email AS account_email
         FROM events e
         JOIN calendars c ON c.id = e.calendar_id
         JOIN accounts a ON a.id = c.account_id
         WHERE e.id = ?1"
    );
    let row = sqlx::query(&sql).bind(id).fetch_optional(pool).await?;
    Ok(row.map(|r| {
        let access_role: String = r.get("access_role");
        let cal_google_id: String = r.get("cal_google_id");
        let account_email: String = r.get("account_email");
        (row_to_event(&r), access_role, cal_google_id, account_email)
    }))
}

/// One event plus its calendar's `access_role`, by row id.
///
/// The role travels alongside the event rather than being a second query the
/// caller makes itself, because the two are only ever needed together: an
/// `EventDetail` cannot decide whether to show RSVP controls from the event
/// row alone.
pub async fn event_by_id(
    pool: &SqlitePool,
    id: i64,
) -> anyhow::Result<Option<(StoredEvent, String)>> {
    Ok(event_row_for_write(pool, id).await?.map(|(ev, role, _, _)| (ev, role)))
}

/// One event plus everything an RSVP write needs beyond it: the calendar's
/// `access_role` (can this calendar be answered at all), the calendar's own
/// `google_id` (which calendar to patch), and the owning account's email
/// (which account's access token to use).
pub async fn event_for_write(
    pool: &SqlitePool,
    id: i64,
) -> anyhow::Result<Option<(StoredEvent, String, String, String)>> {
    event_row_for_write(pool, id).await
}

/// Events overlapping `[from_ms, to_ms)` on selected calendars, plus every
/// recurring master on a selected calendar. Masters are returned unconditionally
/// because their stored `start_utc` is the series start, which may be years
/// before the requested window; expansion happens in `omacal-core::expand`.
///
/// Cancelled rows are excluded — *except* cancelled exceptions. A cancelled
/// exception is the only record that a particular occurrence of a series was
/// deleted; drop it and the master expands into that slot forever. It is
/// returned so the renderer can suppress that occurrence, and renders nothing
/// itself.
///
/// An exception is also matched on `original_start_utc`, not only on its own
/// times: an instance dragged out of the window still has to suppress the slot
/// it left behind.
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
           AND (e.status != 'cancelled' OR e.recurring_event_id IS NOT NULL)
           AND (e.recurrence IS NOT NULL
                OR (e.start_utc < ?2 AND e.end_utc > ?1)
                OR (e.recurring_event_id IS NOT NULL
                    AND e.original_start_utc >= ?1
                    AND e.original_start_utc < ?2))
         ORDER BY e.start_utc"
    );
    let rows = sqlx::query(&sql).bind(from_ms).bind(to_ms).fetch_all(pool).await?;
    Ok(rows.iter().map(row_to_event).collect())
}

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

    /// Two accounts, one calendar each, with the calendar ids and account ids
    /// deliberately crossed so no calendar's `id` equals its own
    /// `account_id`. `seed`'s single account/calendar fixture makes that
    /// coincidence unavoidable — both ids come out `1` — which means a join
    /// on the wrong column can still return the right row by accident. Kept
    /// general enough to seed other cross-account tests, not just this one.
    ///
    /// Returns `(cal_on_account_b, cal_on_account_a)`: the first calendar
    /// inserted (id 1) belongs to the *second* account, and vice versa.
    async fn seed_two_accounts(pool: &SqlitePool) -> (i64, i64) {
        sqlx::query("INSERT INTO accounts (google_sub, email, created_at) VALUES ('a','a@x',0)")
            .execute(pool).await.unwrap();
        sqlx::query("INSERT INTO accounts (google_sub, email, created_at) VALUES ('b','b@x',0)")
            .execute(pool).await.unwrap();
        // Calendar id 1 belongs to account 2, calendar id 2 belongs to
        // account 1 — crossed on purpose.
        sqlx::query(
            "INSERT INTO calendars (account_id, google_id, summary, timezone, access_role)
             VALUES (2, 'cal-on-b', 'On B', 'UTC', 'reader')",
        ).execute(pool).await.unwrap();
        sqlx::query(
            "INSERT INTO calendars (account_id, google_id, summary, timezone, access_role)
             VALUES (1, 'cal-on-a', 'On A', 'UTC', 'owner')",
        ).execute(pool).await.unwrap();
        (1, 2)
    }

    fn ev(cal: i64, gid: &str, start: i64, end: i64) -> StoredEvent {
        StoredEvent {
            id: 0, calendar_id: cal, google_id: gid.into(),
            summary: Some("Standup".into()), location: None,
            start_utc: start, end_utc: end,
            start_tz: "Europe/Sofia".into(), end_tz: "Europe/Sofia".into(),
            is_all_day: false, recurrence: None,
            recurring_event_id: None, original_start_utc: None,
            status: "confirmed".into(),
            self_response: Some("accepted".into()), conference_uri: None,
            color_hex: None,
            description: None, etag: None, sequence: 0, organizer_email: None,
            attendees: Vec::new(),
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

    /// Every column written is read back with the value it went in with. This is
    /// the guard against the upsert's bind order drifting out of step with its
    /// column list — a mismatch there is silent, and lands data in the wrong
    /// column rather than failing.
    #[tokio::test]
    async fn every_column_round_trips_in_its_own_field() {
        let pool = connect_memory().await.unwrap();
        let cal = seed(&pool).await;
        let full = StoredEvent {
            id: 0, calendar_id: cal, google_id: "gid".into(),
            summary: Some("Summary".into()), location: Some("Room 1".into()),
            start_utc: 1_000, end_utc: 2_000,
            start_tz: "Asia/Kolkata".into(), end_tz: "Europe/Sofia".into(),
            is_all_day: true, recurrence: Some("RRULE:FREQ=DAILY".into()),
            recurring_event_id: Some("master".into()), original_start_utc: Some(1_500),
            status: "tentative".into(), self_response: Some("needsAction".into()),
            conference_uri: Some("https://meet/x".into()), color_hex: None,
            description: None, etag: None, sequence: 0, organizer_email: None,
            attendees: Vec::new(),
        };
        upsert_event(&pool, &full).await.unwrap();

        let out = events_in_window(&pool, 0, 5_000).await.unwrap();
        assert_eq!(out.len(), 1);
        let got = &out[0];
        assert_eq!(got.google_id, "gid");
        assert_eq!(got.summary.as_deref(), Some("Summary"));
        assert_eq!(got.location.as_deref(), Some("Room 1"));
        assert_eq!(got.start_utc, 1_000);
        assert_eq!(got.end_utc, 2_000);
        assert_eq!(got.start_tz, "Asia/Kolkata");
        assert_eq!(got.end_tz, "Europe/Sofia");
        assert!(got.is_all_day);
        assert_eq!(got.recurrence.as_deref(), Some("RRULE:FREQ=DAILY"));
        assert_eq!(got.recurring_event_id.as_deref(), Some("master"));
        assert_eq!(got.original_start_utc, Some(1_500));
        assert_eq!(got.status, "tentative");
        assert_eq!(got.self_response.as_deref(), Some("needsAction"));
        assert_eq!(got.conference_uri.as_deref(), Some("https://meet/x"));
    }

    #[tokio::test]
    async fn an_updated_exception_keeps_its_recurrence_link() {
        let pool = connect_memory().await.unwrap();
        let cal = seed(&pool).await;
        let mut x = ev(cal, "x", 1_000, 2_000);
        x.recurring_event_id = Some("master".into());
        x.original_start_utc = Some(1_000);
        upsert_event(&pool, &x).await.unwrap();

        // The instance is dragged later; the original slot must not change.
        x.start_utc = 3_000;
        x.end_utc = 4_000;
        upsert_event(&pool, &x).await.unwrap();

        let out = events_in_window(&pool, 0, 5_000).await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].start_utc, 3_000);
        assert_eq!(out[0].original_start_utc, Some(1_000));
        assert_eq!(out[0].recurring_event_id.as_deref(), Some("master"));
    }

    #[tokio::test]
    async fn an_ordinary_cancelled_event_is_excluded() {
        let pool = connect_memory().await.unwrap();
        let cal = seed(&pool).await;
        let mut e = ev(cal, "a", 1_000, 2_000);
        e.status = "cancelled".into();
        upsert_event(&pool, &e).await.unwrap();
        assert!(events_in_window(&pool, 0, 5_000).await.unwrap().is_empty());
    }

    /// The one cancelled row that must come back: without it the master keeps
    /// expanding into the slot the user deleted.
    #[tokio::test]
    async fn a_cancelled_exception_is_returned_so_it_can_suppress_its_slot() {
        let pool = connect_memory().await.unwrap();
        let cal = seed(&pool).await;
        let mut e = ev(cal, "x", 1_000, 2_000);
        e.status = "cancelled".into();
        e.recurring_event_id = Some("master".into());
        e.original_start_utc = Some(1_000);
        upsert_event(&pool, &e).await.unwrap();

        let out = events_in_window(&pool, 0, 5_000).await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].status, "cancelled");
        assert_eq!(out[0].recurring_event_id.as_deref(), Some("master"));
    }

    /// An instance moved clean out of the window still has to be returned: the
    /// slot it vacated is inside the window and must stay empty.
    #[tokio::test]
    async fn an_exception_moved_out_of_the_window_is_still_returned() {
        let pool = connect_memory().await.unwrap();
        let cal = seed(&pool).await;
        let mut e = ev(cal, "x", 900_000, 901_000);
        e.recurring_event_id = Some("master".into());
        e.original_start_utc = Some(3_000);
        upsert_event(&pool, &e).await.unwrap();

        let out = events_in_window(&pool, 0, 5_000).await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].original_start_utc, Some(3_000));
    }

    #[tokio::test]
    async fn the_calendar_colour_is_joined_onto_each_event() {
        let pool = connect_memory().await.unwrap();
        let cal = seed(&pool).await;
        sqlx::query("UPDATE calendars SET color_hex = '#b58900'")
            .execute(&pool).await.unwrap();
        upsert_event(&pool, &ev(cal, "a", 1000, 2000)).await.unwrap();
        let out = events_in_window(&pool, 0, 5000).await.unwrap();
        assert_eq!(out[0].color_hex.as_deref(), Some("#b58900"));
    }

    #[tokio::test]
    async fn a_calendar_without_a_colour_yields_none() {
        let pool = connect_memory().await.unwrap();
        let cal = seed(&pool).await;
        upsert_event(&pool, &ev(cal, "a", 1000, 2000)).await.unwrap();
        let out = events_in_window(&pool, 0, 5000).await.unwrap();
        assert!(out[0].color_hex.is_none());
    }

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

    // Note: this only checks the DEFAULT applied to a newly-inserted row, not
    // the migration's backfill `UPDATE` — `connect_memory` always starts from
    // an empty database, so there is no pre-existing row for that `UPDATE` to
    // act on. The backfill itself is verified against a copy of the real
    // production database (see task-1-report.md), not by a unit test here.
    #[tokio::test]
    async fn a_new_calendar_defaults_to_synced_and_shown() {
        let pool = connect_memory().await.unwrap();
        let cal = seed(&pool).await;
        let _ = cal;
        let (selected, sync_enabled): (i64, i64) = sqlx::query_as(
            "SELECT selected, sync_enabled FROM calendars LIMIT 1")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(selected, 1);
        assert_eq!(sync_enabled, 1, "a fresh calendar syncs by default");
    }

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

        // The update path: a re-sync of the same google_id must overwrite the
        // stored attendee list, not merge into it or leave it alone. Dropping
        // "ana@x.com" entirely (rather than only editing "me@x.com" in place)
        // also proves the list can shrink, not just mutate an existing entry.
        let mut changed = ev.clone();
        changed.attendees = vec![
            Attendee { email: "me@x.com".into(), display_name: None,
                       response_status: "accepted".into(), optional: true, is_self: true },
        ];
        upsert_event(&pool, &changed).await.unwrap();

        let back2 = events_in_window(&pool, 1786300000000, 1786400000000).await.unwrap();
        let got2 = back2.iter().find(|e| e.google_id == "ev1").expect("event still stored");
        assert_eq!(got2.attendees.len(), 1,
                   "the second upsert must replace the attendee list, not merge it");
        assert_eq!(got2.attendees[0].email, "me@x.com");
        assert_eq!(got2.attendees[0].response_status, "accepted",
                   "the second write's response status did not take effect");
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

    /// Migrations normally all run together at `connect_memory` time, which
    /// would make an assertion here observe only the `DELETE`'s effect, not
    /// the migration actually running against rows that predate it. To
    /// observe the real ordering, this applies 0001 and 0002 by hand, inserts
    /// a pre-upgrade sync cursor, then applies 0003 on top and checks it is
    /// gone.
    #[tokio::test]
    async fn the_migration_drops_every_sync_cursor_so_old_rows_get_backfilled() {
        use sqlx::migrate::{Migration, MigrationSource, Migrator};
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use std::error::Error;
        use std::future::Future;
        use std::pin::Pin;
        use std::str::FromStr;

        /// A fixed list of already-resolved migrations, so a slice of the
        /// real migration set can be run on its own.
        #[derive(Debug)]
        struct Subset(Vec<Migration>);
        impl MigrationSource<'static> for Subset {
            fn resolve(
                self,
            ) -> Pin<
                Box<dyn Future<Output = Result<Vec<Migration>, Box<dyn Error + Sync + Send>>> + Send>,
            > {
                Box::pin(async move { Ok(self.0) })
            }
        }

        let all = sqlx::migrate!("./migrations");
        let opts = SqliteConnectOptions::from_str("sqlite::memory:").unwrap().foreign_keys(true);
        // `max_connections(1)`, same reason as `connect_memory`: every other
        // connection to `:memory:` would be its own empty database.
        let pool = SqlitePoolOptions::new().max_connections(1).connect_with(opts).await.unwrap();

        let pre_upgrade: Vec<Migration> = all.iter().filter(|m| m.version < 3).cloned().collect();
        assert_eq!(pre_upgrade.len(), 2, "expected exactly 0001 and 0002 before 0003 lands");
        Migrator::new(Subset(pre_upgrade)).await.unwrap().run(&pool).await.unwrap();

        let cal = seed(&pool).await;
        sqlx::query("INSERT INTO sync_state (calendar_id, sync_token, window_start, window_end)
                     VALUES (?1, 'tok-from-before-the-upgrade', 0, 0)")
            .bind(cal).execute(&pool).await.unwrap();

        let just_0003: Vec<Migration> = all.iter().filter(|m| m.version == 3).cloned().collect();
        assert_eq!(just_0003.len(), 1, "expected exactly one migration at version 3");
        let mut only_0003 = Migrator::new(Subset(just_0003)).await.unwrap();
        // This source only lists version 3; without this, `run` sees versions
        // 1 and 2 already applied but absent from its own list and refuses to
        // proceed (`VersionMissing`), rather than treating that as fine.
        only_0003.set_ignore_missing(true);
        only_0003.run(&pool).await.unwrap();

        let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sync_state")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(left, 0, "a surviving cursor means old rows never get their attendees");
    }

    #[tokio::test]
    async fn event_by_id_returns_the_event_and_its_calendars_access_role() {
        let pool = connect_memory().await.unwrap();
        let cal = seed(&pool).await;
        let id = upsert_event(&pool, &ev(cal, "a", 1000, 2000)).await.unwrap();

        let (got, access_role) = event_by_id(&pool, id).await.unwrap().expect("event exists");
        assert_eq!(got.google_id, "a");
        assert_eq!(access_role, "owner", "seed()'s calendar is owned");
    }

    #[tokio::test]
    async fn event_by_id_reports_a_read_only_calendars_role() {
        let pool = connect_memory().await.unwrap();
        let cal = seed(&pool).await;
        sqlx::query("UPDATE calendars SET access_role = 'reader' WHERE id = ?1")
            .bind(cal).execute(&pool).await.unwrap();
        let id = upsert_event(&pool, &ev(cal, "a", 1000, 2000)).await.unwrap();

        let (_, access_role) = event_by_id(&pool, id).await.unwrap().expect("event exists");
        assert_eq!(access_role, "reader");
    }

    #[tokio::test]
    async fn event_by_id_returns_none_for_an_unknown_id() {
        let pool = connect_memory().await.unwrap();
        assert!(event_by_id(&pool, 999).await.unwrap().is_none());
    }

    /// Guards the join column itself, not just the value it happens to
    /// produce. With `seed`'s single account/calendar, `c.id` and
    /// `c.account_id` are both 1, so joining on the wrong column returns the
    /// right row by coincidence. `seed_two_accounts` crosses the ids so the
    /// two joins disagree: the event lives on the calendar owned by account
    /// 1, whose *own* `access_role` is `"owner"`, while the calendar that
    /// happens to share its `account_id` with the event's `calendar_id` is a
    /// different row entirely, with `access_role` `"reader"`.
    #[tokio::test]
    async fn event_by_id_returns_the_events_own_calendar_not_one_sharing_its_id() {
        let pool = connect_memory().await.unwrap();
        let (_cal_on_b, cal_on_a) = seed_two_accounts(&pool).await;
        let id = upsert_event(&pool, &ev(cal_on_a, "a", 1000, 2000)).await.unwrap();

        let (got, access_role) = event_by_id(&pool, id).await.unwrap().expect("event exists");
        assert_eq!(got.calendar_id, cal_on_a);
        assert_eq!(access_role, "owner",
            "must be cal_on_a's own role, not a calendar that merely shares an id with it");
    }

    #[tokio::test]
    async fn event_for_write_returns_the_calendars_google_id_and_the_owning_accounts_email() {
        let pool = connect_memory().await.unwrap();
        let cal = seed(&pool).await;
        let id = upsert_event(&pool, &ev(cal, "a", 1000, 2000)).await.unwrap();

        let (got, access_role, cal_google_id, account_email) =
            event_for_write(&pool, id).await.unwrap().expect("event exists");
        assert_eq!(got.google_id, "a", "the event's own google_id must not be shadowed");
        assert_eq!(access_role, "owner");
        assert_eq!(cal_google_id, "primary", "seed()'s calendar google_id");
        assert_eq!(account_email, "e@x", "seed()'s account email");
    }

    #[tokio::test]
    async fn event_for_write_returns_none_for_an_unknown_id() {
        let pool = connect_memory().await.unwrap();
        assert!(event_for_write(&pool, 999).await.unwrap().is_none());
    }

    /// The same guard as `event_by_id_returns_the_events_own_calendar_not_one_sharing_its_id`,
    /// extended to the account join: `seed_two_accounts` crosses calendar and
    /// account ids so joining `accounts` on the wrong column (e.g. `a.id =
    /// c.id` instead of `a.id = c.account_id`) still returns *an* account, just
    /// the wrong one.
    #[tokio::test]
    async fn event_for_write_resolves_the_owning_account_not_one_sharing_an_id() {
        let pool = connect_memory().await.unwrap();
        let (_cal_on_b, cal_on_a) = seed_two_accounts(&pool).await;
        let id = upsert_event(&pool, &ev(cal_on_a, "a", 1000, 2000)).await.unwrap();

        let (_, _, cal_google_id, account_email) =
            event_for_write(&pool, id).await.unwrap().expect("event exists");
        assert_eq!(cal_google_id, "cal-on-a");
        assert_eq!(account_email, "a@x",
            "must be account a's own email, not account b's merely sharing an id with cal_on_a");
    }
}
