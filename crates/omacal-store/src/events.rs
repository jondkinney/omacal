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
