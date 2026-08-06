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
            description: None, etag: None, sequence: 0, organizer_email: None,
            attendees: Vec::new(),
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
