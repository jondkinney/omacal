use serde::Serialize;
use sqlx::{Row, SqlitePool};

#[derive(Debug, Clone, Serialize)]
pub struct CalendarRow {
    pub id: i64,
    pub account_id: i64,
    pub account_email: String,
    pub summary: String,
    /// **The colour to draw this calendar in** — its override if it has one,
    /// and Google's own otherwise, resolved by the same `COALESCE` the event
    /// read uses. Every consumer that only wants to *draw* something reads
    /// this and needs to know nothing about overrides.
    pub color_hex: Option<String>,
    /// The override itself, or `None` when there is not one.
    ///
    /// Separate from the field above because **clearing an override is a
    /// different state from setting it to whatever Google currently uses**,
    /// and only this field can tell them apart — the settings row needs it to
    /// show which swatch is chosen and whether there is anything to clear. See
    /// `0006_calendar_colour.sql`.
    pub color_override: Option<String>,
    /// Drawn in the grid.
    pub selected: bool,
    /// Fetched from Google at all.
    pub sync_enabled: bool,
    pub is_primary: bool,
    /// Google's own word for what this account may do here: `owner`, `writer`,
    /// `reader`, `freeBusyReader`.
    ///
    /// Carried all the way to the UI because the event form has to offer only
    /// the calendars a create could actually land on — a subscribed holiday
    /// calendar is a `reader`, and offering it produces a Save that
    /// `create_impl` can only refuse. That refusal already exists and stays
    /// (`can_edit`, applied server-side against this same column via
    /// `calendar_for_write`); this field is what stops the UI walking into it.
    pub access_role: String,
    /// The owning account's provider (`google` | `caldav`) — what the UI
    /// gates provider-specific affordances on (event editing is
    /// Google-only until the CalDAV write phase lands).
    pub provider: String,
}

/// Every calendar across every account, primary first, then alphabetical —
/// stable ordering so the popover does not reshuffle between renders.
pub async fn list_calendars(pool: &SqlitePool) -> anyhow::Result<Vec<CalendarRow>> {
    let rows = sqlx::query(
        "SELECT c.id, c.account_id, a.email AS account_email, c.summary,
                COALESCE(c.color_override, c.color_hex) AS color_hex,
                c.color_override,
                c.selected, c.sync_enabled, c.is_primary, c.access_role, a.provider
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
            color_override: r.get("color_override"),
            selected: r.get::<i64, _>("selected") != 0,
            sync_enabled: r.get::<i64, _>("sync_enabled") != 0,
            is_primary: r.get::<i64, _>("is_primary") != 0,
            access_role: r.get("access_role"),
            provider: r.get("provider"),
        })
        .collect())
}

/// One calendar's `google_id`, `access_role`, owning account's email, and own
/// stored `timezone`, by the calendar's own row id.
///
/// The counterpart to [`crate::events::event_for_write`] for a write that
/// creates an event rather than changing one that already exists: there is no
/// event row yet to key a lookup on, only the calendar it will be created on,
/// so this starts from `calendars` instead of `events` and skips straight to
/// the two joins that query already does.
///
/// `timezone` is included because it must be the *calendar's own* zone that a
/// caller later hands to `omacal_sync::to_stored`, not whatever zone the
/// caller happens to be authoring an event in — see `create_via_client`'s doc
/// comment for why an all-day create is the case that makes those two differ.
pub async fn calendar_for_write(
    pool: &SqlitePool,
    id: i64,
) -> anyhow::Result<Option<(String, String, String, String)>> {
    let row = sqlx::query(
        "SELECT c.google_id, c.access_role, a.email AS account_email, c.timezone
         FROM calendars c
         JOIN accounts a ON a.id = c.account_id
         WHERE c.id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| {
        (r.get("google_id"), r.get("access_role"), r.get("account_email"), r.get("timezone"))
    }))
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

/// Sets or clears a calendar's colour override.
///
/// `None` **clears** it, which is not the same as storing whatever Google
/// currently uses: a cleared calendar follows Google's colour from then on,
/// including when Google changes it. See `0006_calendar_colour.sql`.
///
/// Nothing about this reaches Google. It is a display preference of this
/// install's, and the phone, the web UI and anyone sharing the calendar are
/// untouched by it.
pub async fn set_color_override(
    pool: &SqlitePool,
    id: i64,
    hex: Option<&str>,
) -> anyhow::Result<()> {
    sqlx::query("UPDATE calendars SET color_override = ?2 WHERE id = ?1")
        .bind(id)
        .bind(hex)
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
        // The invite ledger goes with the events — through them, so before
        // them. Left behind, a stale `invite_scan` row would make a re-added
        // calendar's backlog read as news, and orphaned notices could
        // silence a fresh invitation if SQLite ever reissued a rowid.
        sqlx::query(
            "DELETE FROM invite_notices WHERE event_id IN (
                SELECT id FROM events WHERE calendar_id = ?1)",
        )
        .bind(id)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM invite_scan WHERE calendar_id = ?1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        let n = sqlx::query("DELETE FROM events WHERE calendar_id = ?1")
            .bind(id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        // AFTER the events delete, in the same transaction: the 0011 delete
        // trigger just recorded every one of those rows as "cancelled", and
        // a removed calendar is not a hundred cancellations.
        sqlx::query("DELETE FROM event_changes WHERE calendar_id = ?1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
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

    /// A pool with the two seeded calendars, which is what every colour test
    /// below starts from.
    async fn seeded() -> SqlitePool {
        let pool = connect_memory().await.unwrap();
        seed(&pool).await;
        pool
    }

    fn ev(cal: i64, gid: &str) -> StoredEvent {
        StoredEvent {
            id: 0, calendar_id: cal, google_id: gid.into(), summary: Some("x".into()),
            location: None, start_utc: 1000, end_utc: 2000,
            start_tz: "UTC".into(), end_tz: "UTC".into(), is_all_day: false,
            recurrence: None, recurring_event_id: None, original_start_utc: None,
            status: "confirmed".into(), self_response: None, conference_uri: None,
            color_hex: None, calendar_timezone: "UTC".into(),
            description: None, etag: None, sequence: 0, organizer_email: None,
            guests_can_modify: false,
            attendees: Vec::new(),
            reminders: Default::default(), calendar_default_reminders: Vec::new(),
        }
    }

    #[tokio::test]
    async fn calendar_for_write_returns_the_calendars_google_id_role_email_and_timezone() {
        let pool = connect_memory().await.unwrap();
        seed(&pool).await;
        let (google_id, access_role, account_email, timezone) =
            calendar_for_write(&pool, 1).await.unwrap().expect("calendar exists");
        assert_eq!(google_id, "primary");
        assert_eq!(access_role, "owner");
        assert_eq!(account_email, "me@x.com");
        assert_eq!(timezone, "UTC", "seed()'s calendar timezone");
    }

    #[tokio::test]
    async fn calendar_for_write_returns_none_for_an_unknown_id() {
        let pool = connect_memory().await.unwrap();
        // Seeded first, so this proves the `WHERE` clause actually filters —
        // against a bare empty pool the same assertion would pass whether or
        // not the query filters on `id` at all.
        seed(&pool).await;
        assert!(calendar_for_write(&pool, 999).await.unwrap().is_none());
    }

    /// Crosses calendar and account ids the same way
    /// `omacal_store::events::tests::seed_two_accounts` does — calendar id 1
    /// belongs to account 2, calendar id 2 belongs to account 1 — so a join on
    /// the wrong column (`a.id = c.id` instead of `a.id = c.account_id`)
    /// still returns *an* account row, just the wrong one, rather than
    /// failing loudly. Returns the id of the calendar owned by account "a".
    async fn seed_two_accounts(pool: &SqlitePool) -> i64 {
        sqlx::query("INSERT INTO accounts (google_sub, email, created_at) VALUES ('a','a@x',0)")
            .execute(pool).await.unwrap();
        sqlx::query("INSERT INTO accounts (google_sub, email, created_at) VALUES ('b','b@x',0)")
            .execute(pool).await.unwrap();
        sqlx::query(
            "INSERT INTO calendars (account_id, google_id, summary, timezone, access_role)
             VALUES (2, 'cal-on-b', 'On B', 'UTC', 'reader')",
        ).execute(pool).await.unwrap();
        sqlx::query(
            "INSERT INTO calendars (account_id, google_id, summary, timezone, access_role)
             VALUES (1, 'cal-on-a', 'On A', 'Pacific/Auckland', 'owner')",
        ).execute(pool).await.unwrap();

        let (id, account_id): (i64, i64) =
            sqlx::query_as("SELECT id, account_id FROM calendars WHERE google_id = 'cal-on-a'")
                .fetch_one(pool)
                .await
                .unwrap();
        debug_assert_ne!(
            id, account_id,
            "cal-on-a: the calendar's id must not equal its own account_id, or a join on the \
             wrong column still returns the right row"
        );
        id
    }

    /// The account JOIN, unbound without this: `seed_two_accounts` crosses
    /// calendar and account ids so joining `accounts` on the wrong column
    /// (e.g. `a.id = c.id` instead of `a.id = c.account_id`) still returns an
    /// account, just account "b"'s instead of "a"'s. `account_email` is what
    /// `access_token_for` uses to pick a Google account's token, so a wrong
    /// join here means creating the event under a different Google account
    /// than the calendar's own.
    #[tokio::test]
    async fn calendar_for_write_resolves_the_owning_account_not_one_sharing_an_id() {
        let pool = connect_memory().await.unwrap();
        let cal_on_a = seed_two_accounts(&pool).await;

        let (google_id, _access_role, account_email, _timezone) =
            calendar_for_write(&pool, cal_on_a).await.unwrap().expect("calendar exists");
        assert_eq!(google_id, "cal-on-a");
        assert_eq!(
            account_email, "a@x",
            "must be account a's own email, not account b's merely sharing an id with cal_on_a"
        );
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

    /// The event form has nothing but this column to decide which calendars it
    /// may offer, and the query dropped it until Task 9 — every calendar
    /// reached the UI looking equally writable, subscribed holiday calendars
    /// included. `seed_two_accounts` seeds one `owner` and one `reader`, so a
    /// query that hard-coded either value still fails here.
    #[tokio::test]
    async fn listing_reports_each_calendars_access_role() {
        let pool = connect_memory().await.unwrap();
        seed_two_accounts(&pool).await;
        let cals = list_calendars(&pool).await.unwrap();
        let role = |summary: &str| {
            cals.iter()
                .find(|c| c.summary == summary)
                .unwrap_or_else(|| panic!("no calendar named {summary}"))
                .access_role
                .clone()
        };
        assert_eq!(role("On A"), "owner");
        assert_eq!(role("On B"), "reader");
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

    /// **The colour to draw is the override when there is one, and Google's
    /// otherwise** — resolved in the `SELECT`, which is why nothing downstream
    /// of it knows an override exists.
    #[tokio::test]
    async fn an_override_is_the_colour_the_list_reports() {
        let pool = seeded().await;
        let before = list_calendars(&pool).await.unwrap();
        let id = before[0].id;
        assert_eq!(before[0].color_hex.as_deref(), Some("#5b8def"), "Google's own");
        assert_eq!(before[0].color_override, None, "nothing chosen yet");

        set_color_override(&pool, id, Some("#e2a03f")).await.unwrap();

        let after = list_calendars(&pool).await.unwrap();
        assert_eq!(after[0].color_hex.as_deref(), Some("#e2a03f"), "the colour to draw");
        assert_eq!(after[0].color_override.as_deref(), Some("#e2a03f"), "and it is a choice");
    }

    /// **Clearing is not the same as choosing Google's current colour**, and
    /// this is the test that says so: after a clear, a *change on Google's
    /// side* is followed. Store the colour instead of a NULL and the calendar
    /// silently stops following it, with nothing recording which the user
    /// meant.
    #[tokio::test]
    async fn a_cleared_override_follows_google_again_even_when_google_changes() {
        let pool = seeded().await;
        let id = list_calendars(&pool).await.unwrap()[0].id;
        set_color_override(&pool, id, Some("#e2a03f")).await.unwrap();

        set_color_override(&pool, id, None).await.unwrap();
        let cleared = list_calendars(&pool).await.unwrap();
        assert_eq!(cleared[0].color_hex.as_deref(), Some("#5b8def"));
        assert_eq!(cleared[0].color_override, None);

        // The half that a stored-copy implementation passes right up until
        // here: Google recolours the calendar on its next sign-in.
        sqlx::query("UPDATE calendars SET color_hex = '#b58900' WHERE id = ?1")
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();

        let followed = list_calendars(&pool).await.unwrap();
        assert_eq!(
            followed[0].color_hex.as_deref(),
            Some("#b58900"),
            "a cleared calendar must follow Google's colour, including its changes",
        );
    }

    /// And an override does **not** follow Google — that is what choosing one
    /// means, and without this the rule above is satisfied by never storing an
    /// override at all.
    #[tokio::test]
    async fn an_override_survives_google_changing_its_own_colour() {
        let pool = seeded().await;
        let id = list_calendars(&pool).await.unwrap()[0].id;
        set_color_override(&pool, id, Some("#e2a03f")).await.unwrap();

        sqlx::query("UPDATE calendars SET color_hex = '#b58900' WHERE id = ?1")
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();

        assert_eq!(list_calendars(&pool).await.unwrap()[0].color_hex.as_deref(), Some("#e2a03f"));
    }
}

/// Removes one account and everything it owned: its calendars, their events,
/// tasks and sync cursors through the schema's cascades — and its
/// fired-reminder records by hand, because that table deliberately carries no
/// foreign key (the scheduler prunes it by time instead). Left behind, those
/// rows could suppress a reminder if SQLite ever handed a new event the same
/// rowid. Returns how many account rows went (0 or 1) — the caller decides
/// whether 0 is an error.
///
/// Lives in the store rather than in a command so the removal chain is
/// testable against the real schema: a migration that broke a CASCADE would
/// otherwise only be discovered by a user with orphaned rows.
pub async fn delete_account(pool: &SqlitePool, account_id: i64) -> anyhow::Result<u64> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "DELETE FROM fired_reminders WHERE event_id IN (
            SELECT e.id FROM events e
            JOIN calendars c ON c.id = e.calendar_id
            WHERE c.account_id = ?1)",
    )
    .bind(account_id)
    .execute(&mut *tx)
    .await?;
    // The invite ledger carries no foreign keys either, for the same reason
    // as fired_reminders — so the same by-hand sweep, and through the events
    // before the cascade takes them.
    sqlx::query(
        "DELETE FROM invite_notices WHERE event_id IN (
            SELECT e.id FROM events e
            JOIN calendars c ON c.id = e.calendar_id
            WHERE c.account_id = ?1)",
    )
    .bind(account_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "DELETE FROM invite_scan WHERE calendar_id IN (
            SELECT id FROM calendars WHERE account_id = ?1)",
    )
    .bind(account_id)
    .execute(&mut *tx)
    .await?;
    // The calendar ids, captured before the cascade takes the rows they
    // would be read from — the change-ledger sweep below needs them *after*
    // the cascade's deletes have fired the 0011 trigger.
    let calendar_ids: Vec<i64> =
        sqlx::query_scalar("SELECT id FROM calendars WHERE account_id = ?1")
            .bind(account_id)
            .fetch_all(&mut *tx)
            .await?;
    let gone = sqlx::query("DELETE FROM accounts WHERE id = ?1")
        .bind(account_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    // Same reasoning as set_sync_enabled's sweep: the cascade just recorded
    // every deleted event as "cancelled", and signing out is not that.
    for id in calendar_ids {
        sqlx::query("DELETE FROM event_changes WHERE calendar_id = ?1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(gone)
}

#[cfg(test)]
mod cascade_tests {
    use super::*;

    #[tokio::test]
    async fn deleting_an_account_cascades_through_everything_it_owned() {
        let pool = crate::connect_memory().await.unwrap();
        sqlx::query(
            "INSERT INTO accounts (google_sub, email, created_at, provider)
             VALUES ('caldav:x', 'x@x', 0, 'caldav')",
        )
        .execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO calendars (account_id, google_id, summary, timezone, access_role, supports_tasks)
             VALUES (1, 'url', 'Cal', 'UTC', 'owner', 1)",
        )
        .execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO events (calendar_id, google_id, start_utc, end_utc, start_tz, end_tz, updated_at)
             VALUES (1, 'ev', 0, 1, 'UTC', 'UTC', 0)",
        )
        .execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO tasks (calendar_id, uid, updated_at) VALUES (1, 't', 0)",
        )
        .execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO sync_state (calendar_id, sync_token, window_start, window_end)
             VALUES (1, 'ctag', 0, 1)",
        )
        .execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO fired_reminders (event_id, occurrence_ms, minutes, occurrence_end_ms, fired_at_ms)
             VALUES (1, 0, 5, 1, 0)",
        )
        .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO invite_notices (event_id, noticed_at_ms, posted) VALUES (1, 0, 1)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO invite_scan (calendar_id, seeded_at_ms) VALUES (1, 0)")
            .execute(&pool).await.unwrap();

        // An event the cascade will delete — whatever 0011's delete trigger
        // records about it, the sweep must take back out.
        assert_eq!(delete_account(&pool, 1).await.unwrap(), 1);
        for table in ["accounts", "calendars", "events", "tasks", "sync_state", "fired_reminders",
                      "invite_notices", "invite_scan", "event_changes"] {
            let n: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
                .fetch_one(&pool).await.unwrap();
            assert_eq!(n, 0, "{table} should be empty after the cascade");
        }
        assert_eq!(delete_account(&pool, 1).await.unwrap(), 0, "second delete finds nothing");
    }
}
