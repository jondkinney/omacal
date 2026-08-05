pub mod convert;
pub use convert::{is_tombstone, to_cancelled_exception, to_stored};

use omacal_google::{ApiError, CalendarClient, EventsRequest};
use sqlx::SqlitePool;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct SyncOutcome {
    pub upserted: usize,
    pub deleted: usize,
    pub did_full_resync: bool,
}

fn to_rfc3339(ms: i64) -> String {
    jiff::Timestamp::from_millisecond(ms)
        .map(|t| t.to_string())
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

/// Syncs one calendar, following pagination and recovering from a stale token.
///
/// Uses the stored `sync_token` when present. On `410 GONE` the token is
/// discarded and a full windowed sync runs instead — expected behaviour, not an
/// error (spec §5).
pub async fn sync_calendar(
    pool: &SqlitePool,
    client: &CalendarClient,
    calendar_id: i64,
    google_id: &str,
    window_start_ms: i64,
    window_end_ms: i64,
) -> anyhow::Result<SyncOutcome> {
    let cal_tz: String =
        sqlx::query_scalar("SELECT timezone FROM calendars WHERE id = ?1")
            .bind(calendar_id)
            .fetch_one(pool)
            .await?;

    let stored_token: Option<String> =
        sqlx::query_scalar("SELECT sync_token FROM sync_state WHERE calendar_id = ?1")
            .bind(calendar_id)
            .fetch_optional(pool)
            .await?
            .flatten();

    let mut outcome = SyncOutcome::default();
    let mut token = stored_token;

    loop {
        match drain(pool, client, calendar_id, google_id, &cal_tz,
                    token.clone(), window_start_ms, window_end_ms, &mut outcome).await
        {
            Ok(next) => {
                // COALESCE, not a plain assignment: a page can legitimately end
                // without a `nextSyncToken`, and overwriting a good token with
                // NULL would silently downgrade every later sync to a full
                // window fetch.
                sqlx::query(
                    "INSERT INTO sync_state (calendar_id, sync_token, window_start, window_end)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT (calendar_id) DO UPDATE SET
                         sync_token = COALESCE(excluded.sync_token, sync_state.sync_token),
                         window_start = excluded.window_start,
                         window_end = excluded.window_end",
                )
                .bind(calendar_id)
                .bind(&next)
                .bind(window_start_ms)
                .bind(window_end_ms)
                .execute(pool)
                .await?;
                return Ok(outcome);
            }
            Err(ApiError::SyncTokenInvalid) if token.is_some() => {
                tracing::warn!(calendar_id, "sync token rejected, falling back to full resync");
                outcome = SyncOutcome { did_full_resync: true, ..Default::default() };
                token = None;
                continue;
            }
            Err(e) => return Err(e.into()),
        }
    }
}

/// Walks every page for one attempt, returning the final `nextSyncToken`.
#[allow(clippy::too_many_arguments)]
async fn drain(
    pool: &SqlitePool,
    client: &CalendarClient,
    calendar_id: i64,
    google_id: &str,
    cal_tz: &str,
    sync_token: Option<String>,
    window_start_ms: i64,
    window_end_ms: i64,
    outcome: &mut SyncOutcome,
) -> Result<Option<String>, ApiError> {
    let mut page_token: Option<String> = None;

    loop {
        let req = EventsRequest {
            sync_token: sync_token.clone(),
            time_min: sync_token.is_none().then(|| to_rfc3339(window_start_ms)),
            time_max: sync_token.is_none().then(|| to_rfc3339(window_end_ms)),
            page_token: page_token.clone(),
        };
        let page = client.list_events(google_id, &req).await?;

        for ev in &page.events {
            if is_tombstone(ev) {
                // A tombstone for one occurrence of a series is not a deletion
                // of any stored row — the occurrence exists only as an
                // expansion of its master. Store it, so the renderer knows to
                // leave that slot empty. Everything else really is a deletion.
                if let Some(row) = to_cancelled_exception(ev, calendar_id, cal_tz) {
                    if omacal_store::upsert_event(pool, &row).await.is_ok() {
                        outcome.upserted += 1;
                    }
                } else if omacal_store::delete_event(pool, calendar_id, &ev.id).await.is_ok() {
                    outcome.deleted += 1;
                }
            } else if let Some(stored) = to_stored(ev, calendar_id, cal_tz) {
                if omacal_store::upsert_event(pool, &stored).await.is_ok() {
                    outcome.upserted += 1;
                }
            } else {
                tracing::warn!(event_id = %ev.id, "skipping unparseable event");
            }
        }

        match page.next_page_token {
            Some(t) => page_token = Some(t),
            None => return Ok(page.next_sync_token),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omacal_google::CalendarClient;
    use wiremock::matchers::{method, path, query_param, query_param_is_missing};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn seeded_pool() -> sqlx::SqlitePool {
        let pool = omacal_store::connect_memory().await.unwrap();
        sqlx::query("INSERT INTO accounts (google_sub, email, created_at) VALUES ('s','e@x',0)")
            .execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO calendars (account_id, google_id, summary, timezone, access_role)
             VALUES (1, 'primary', 'Work', 'Europe/Sofia', 'owner')",
        ).execute(&pool).await.unwrap();
        pool
    }

    fn stored(gid: &str, start: i64, end: i64) -> omacal_store::StoredEvent {
        omacal_store::StoredEvent {
            id: 0, calendar_id: 1, google_id: gid.into(), summary: Some("Standup".into()),
            location: None, start_utc: start, end_utc: end,
            start_tz: "Europe/Sofia".into(), end_tz: "Europe/Sofia".into(),
            is_all_day: false, recurrence: None,
            recurring_event_id: None, original_start_utc: None,
            status: "confirmed".into(), self_response: None, conference_uri: None,
            color_hex: None,
        }
    }

    fn one_event_body(token: &str) -> serde_json::Value {
        serde_json::json!({
            "items": [{
                "id": "e1", "status": "confirmed", "summary": "Standup",
                "start": {"dateTime": "2026-08-03T09:00:00+03:00", "timeZone": "Europe/Sofia"},
                "end":   {"dateTime": "2026-08-03T09:30:00+03:00", "timeZone": "Europe/Sofia"}
            }],
            "nextSyncToken": token
        })
    }

    #[tokio::test]
    async fn a_first_sync_stores_events_and_records_the_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/calendars/primary/events"))
            .respond_with(ResponseTemplate::new(200).set_body_json(one_event_body("tok-1")))
            .mount(&server).await;

        let pool = seeded_pool().await;
        let client = CalendarClient::new(server.uri(), "at-1");
        let out = sync_calendar(&pool, &client, 1, "primary", 0, 9_999_999_999_999).await.unwrap();

        assert_eq!(out.upserted, 1);
        assert!(!out.did_full_resync);
        let tok: Option<String> = sqlx::query_scalar(
            "SELECT sync_token FROM sync_state WHERE calendar_id = 1")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(tok.as_deref(), Some("tok-1"));
    }

    #[tokio::test]
    async fn a_tombstone_deletes_the_local_row() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/calendars/primary/events"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [{"id": "e1", "status": "cancelled"}],
                "nextSyncToken": "tok-2"
            })))
            .mount(&server).await;

        let pool = seeded_pool().await;
        omacal_store::upsert_event(&pool, &stored("e1", 1000, 2000)).await.unwrap();

        let client = CalendarClient::new(server.uri(), "at-1");
        let out = sync_calendar(&pool, &client, 1, "primary", 0, 9_999_999_999_999).await.unwrap();
        assert_eq!(out.deleted, 1);
        assert!(omacal_store::events_in_window(&pool, 0, 5000).await.unwrap().is_empty());
    }

    /// The recovery path from spec §5. A stale token must not be fatal.
    #[tokio::test]
    async fn a_410_triggers_a_full_resync() {
        let server = MockServer::start().await;
        // With a syncToken: 410.
        Mock::given(method("GET")).and(path("/calendars/primary/events"))
            .and(query_param("syncToken", "stale"))
            .respond_with(ResponseTemplate::new(410))
            .mount(&server).await;
        // Without one: succeed.
        Mock::given(method("GET")).and(path("/calendars/primary/events"))
            .and(query_param("singleEvents", "false"))
            .respond_with(ResponseTemplate::new(200).set_body_json(one_event_body("tok-fresh")))
            .mount(&server).await;

        let pool = seeded_pool().await;
        sqlx::query(
            "INSERT INTO sync_state (calendar_id, sync_token, window_start, window_end)
             VALUES (1, 'stale', 0, 0)")
            .execute(&pool).await.unwrap();

        let client = CalendarClient::new(server.uri(), "at-1");
        let out = sync_calendar(&pool, &client, 1, "primary", 0, 9_999_999_999_999).await.unwrap();

        assert!(out.did_full_resync);
        assert_eq!(out.upserted, 1);
        let tok: Option<String> = sqlx::query_scalar(
            "SELECT sync_token FROM sync_state WHERE calendar_id = 1")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(tok.as_deref(), Some("tok-fresh"));
    }

    /// Page 1: an event plus `nextPageToken`, no `nextSyncToken` (Google never
    /// sends a sync token until the last page). Page 2, matched on that
    /// `pageToken`: a different event, no `nextPageToken`, and the real
    /// `nextSyncToken`.
    fn page_one_body(next_page_token: &str, decoy_sync_token: Option<&str>) -> serde_json::Value {
        let mut body = serde_json::json!({
            "items": [{
                "id": "e1", "status": "confirmed", "summary": "Standup",
                "start": {"dateTime": "2026-08-03T09:00:00+03:00", "timeZone": "Europe/Sofia"},
                "end":   {"dateTime": "2026-08-03T09:30:00+03:00", "timeZone": "Europe/Sofia"}
            }],
            "nextPageToken": next_page_token
        });
        if let Some(t) = decoy_sync_token {
            body["nextSyncToken"] = serde_json::json!(t);
        }
        body
    }

    fn page_two_body(sync_token: &str) -> serde_json::Value {
        serde_json::json!({
            "items": [{
                "id": "e2", "status": "confirmed", "summary": "Retro",
                "start": {"dateTime": "2026-08-04T09:00:00+03:00", "timeZone": "Europe/Sofia"},
                "end":   {"dateTime": "2026-08-04T09:30:00+03:00", "timeZone": "Europe/Sofia"}
            }],
            "nextSyncToken": sync_token
        })
    }

    #[tokio::test]
    async fn a_multi_page_sync_consumes_every_page() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/calendars/primary/events"))
            .and(query_param_is_missing("pageToken"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page_one_body("page-2-token", None)))
            .mount(&server).await;
        Mock::given(method("GET")).and(path("/calendars/primary/events"))
            .and(query_param("pageToken", "page-2-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page_two_body("tok-final")))
            .mount(&server).await;

        let pool = seeded_pool().await;
        let client = CalendarClient::new(server.uri(), "at-1");
        let out = sync_calendar(&pool, &client, 1, "primary", 0, 9_999_999_999_999).await.unwrap();

        assert_eq!(out.upserted, 2);
        let stored = omacal_store::events_in_window(&pool, 0, 9_999_999_999_999).await.unwrap();
        assert_eq!(stored.len(), 2);
        let ids: Vec<&str> = stored.iter().map(|e| e.google_id.as_str()).collect();
        assert!(ids.contains(&"e1"));
        assert!(ids.contains(&"e2"));

        let tok: Option<String> = sqlx::query_scalar(
            "SELECT sync_token FROM sync_state WHERE calendar_id = 1")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(tok.as_deref(), Some("tok-final"));
    }

    /// A sharper regression guard than the test above: page 1 also carries a
    /// `nextSyncToken`, as it would if a caller ever changed `drain` to return
    /// on the first token it sees rather than looping until pagination ends.
    /// Only the token from the *last* page may ever be stored.
    #[tokio::test]
    async fn the_sync_token_comes_from_the_final_page_only() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/calendars/primary/events"))
            .and(query_param_is_missing("pageToken"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                page_one_body("page-2-token", Some("WRONG-do-not-store"))))
            .mount(&server).await;
        Mock::given(method("GET")).and(path("/calendars/primary/events"))
            .and(query_param("pageToken", "page-2-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(page_two_body("tok-final")))
            .mount(&server).await;

        let pool = seeded_pool().await;
        let client = CalendarClient::new(server.uri(), "at-1");
        sync_calendar(&pool, &client, 1, "primary", 0, 9_999_999_999_999).await.unwrap();

        let tok: Option<String> = sqlx::query_scalar(
            "SELECT sync_token FROM sync_state WHERE calendar_id = 1")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(tok.as_deref(), Some("tok-final"));
        assert_ne!(tok.as_deref(), Some("WRONG-do-not-store"));
    }

    /// Deleting one occurrence of a series arrives as a cancelled row carrying
    /// `recurringEventId`. Deleting by that id is a no-op — no such row was ever
    /// stored — and the master would go on expanding into the slot. It has to be
    /// stored instead.
    #[tokio::test]
    async fn a_cancelled_exception_is_stored_rather_than_deleted() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/calendars/primary/events"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [{
                    "id": "master_20260804T060000Z",
                    "status": "cancelled",
                    "recurringEventId": "master",
                    "originalStartTime": {
                        "dateTime": "2026-08-04T09:00:00+03:00",
                        "timeZone": "Europe/Sofia"
                    }
                }],
                "nextSyncToken": "tok-x"
            })))
            .mount(&server).await;

        let pool = seeded_pool().await;
        let client = CalendarClient::new(server.uri(), "at-1");
        let out = sync_calendar(&pool, &client, 1, "primary", 0, 9_999_999_999_999).await.unwrap();

        assert_eq!(out.deleted, 0, "an exception tombstone is not a deletion");
        assert_eq!(out.upserted, 1);

        let rows = omacal_store::events_in_window(&pool, 0, 9_999_999_999_999).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, "cancelled");
        assert_eq!(rows[0].recurring_event_id.as_deref(), Some("master"));
        assert_eq!(rows[0].original_start_utc, Some(1_785_823_200_000));
    }

    /// The other half of the pair: a tombstone with no `recurringEventId` is an
    /// ordinary deletion and must still delete.
    #[tokio::test]
    async fn a_tombstone_without_a_master_still_deletes() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/calendars/primary/events"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [{"id": "e1", "status": "cancelled"}],
                "nextSyncToken": "tok-y"
            })))
            .mount(&server).await;

        let pool = seeded_pool().await;
        omacal_store::upsert_event(&pool, &stored("e1", 1000, 2000)).await.unwrap();

        let client = CalendarClient::new(server.uri(), "at-1");
        let out = sync_calendar(&pool, &client, 1, "primary", 0, 9_999_999_999_999).await.unwrap();

        assert_eq!(out.deleted, 1);
        assert_eq!(out.upserted, 0);
        assert!(omacal_store::events_in_window(&pool, 0, 9_999_999_999_999)
            .await.unwrap().is_empty());
    }

    /// A page may legitimately end without a `nextSyncToken`. Writing that
    /// `None` over a good token would silently downgrade every later sync to a
    /// full window fetch.
    #[tokio::test]
    async fn a_missing_sync_token_does_not_erase_the_stored_one() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/calendars/primary/events"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [{
                    "id": "e1", "status": "confirmed", "summary": "Standup",
                    "start": {"dateTime": "2026-08-03T09:00:00+03:00"},
                    "end":   {"dateTime": "2026-08-03T09:30:00+03:00"}
                }]
                // No nextSyncToken, and no nextPageToken: this is the last page.
            })))
            .mount(&server).await;

        let pool = seeded_pool().await;
        sqlx::query(
            "INSERT INTO sync_state (calendar_id, sync_token, window_start, window_end)
             VALUES (1, 'tok-good', 0, 0)")
            .execute(&pool).await.unwrap();

        let client = CalendarClient::new(server.uri(), "at-1");
        sync_calendar(&pool, &client, 1, "primary", 0, 9_999_999_999_999).await.unwrap();

        let tok: Option<String> = sqlx::query_scalar(
            "SELECT sync_token FROM sync_state WHERE calendar_id = 1")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(tok.as_deref(), Some("tok-good"), "a good token was overwritten with NULL");
    }
}
