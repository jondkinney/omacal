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
}

const SELECT_COLS: &str = "e.id, e.calendar_id, e.google_id, e.summary, e.location,
     e.start_utc, e.end_utc, e.start_tz, e.end_tz, e.is_all_day, e.recurrence,
     e.recurring_event_id, e.original_start_utc,
     e.status, e.self_response, e.conference_uri, c.color_hex";

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
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub async fn upsert_event(pool: &SqlitePool, ev: &StoredEvent) -> anyhow::Result<i64> {
    // 16 columns, 16 placeholders, 16 binds, all in the same order. Keep them
    // that way: a mismatch here writes a value into the wrong column silently.
    let id: i64 = sqlx::query(
        "INSERT INTO events (calendar_id, google_id, summary, location, start_utc, end_utc,
             start_tz, end_tz, is_all_day, recurrence, recurring_event_id,
             original_start_utc, status, self_response, conference_uri, updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)
         ON CONFLICT (calendar_id, google_id) DO UPDATE SET
             summary = excluded.summary, location = excluded.location,
             start_utc = excluded.start_utc, end_utc = excluded.end_utc,
             start_tz = excluded.start_tz, end_tz = excluded.end_tz,
             is_all_day = excluded.is_all_day, recurrence = excluded.recurrence,
             recurring_event_id = excluded.recurring_event_id,
             original_start_utc = excluded.original_start_utc,
             status = excluded.status, self_response = excluded.self_response,
             conference_uri = excluded.conference_uri, updated_at = excluded.updated_at
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
}
