use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

pub mod calendars;
pub mod changes;
pub mod declines;
pub mod events;
pub mod invites;
pub mod reminders;
pub mod tasks;
pub use calendars::{
    calendar_for_write, delete_account, list_calendars, set_color_override, set_selected,
    set_sync_enabled, CalendarRow,
};
pub use changes::{
    changed_meetings, dismiss_all_changes, dismiss_change, forget_changes, ChangedMeeting,
};
pub use declines::{declined_guests, dismiss_all_declines, dismiss_decline, DeclinedGuest};
pub use invites::{
    mark_invites_seeded, pending_invites, record_invite_notice, unanswered_invites,
    unseeded_calendars, InviteCandidate,
};
pub use reminders::{fired_keys, prune_fired, record_fired};
pub use tasks::{
    delete_task, delete_tasks_not_in, mark_task_status, task_by_id, tasks_for_ui, upsert_task,
    StoredTask, TaskRow,
};
pub use events::{
    delete_event, delete_series, event_by_id, event_for_write, events_in_window, exceptions_from,
    known_guests, search_events, upsert_event, Attendee, KnownGuest, Reminder, Reminders,
    StoredEvent,
};

/// Opens (creating if needed) the database at `url` and runs migrations.
pub async fn connect(url: &str) -> anyhow::Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str(url)?
        .create_if_missing(true)
        .foreign_keys(true)
        // WAL keeps the UI's reads from blocking the sync task's writes.
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);
    let pool = SqlitePoolOptions::new().max_connections(5).connect_with(opts).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

/// An isolated in-memory database for tests. `max_connections(1)` is required:
/// each new connection to `:memory:` would otherwise get its own empty database.
pub async fn connect_memory() -> anyhow::Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);
    let pool = SqlitePoolOptions::new().max_connections(1).connect_with(opts).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn migrations_apply_to_a_fresh_database() {
        let pool = connect_memory().await.unwrap();
        let tables: Vec<(String,)> = sqlx::query_as(
            "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        let names: Vec<String> = tables.into_iter().map(|t| t.0).collect();
        for expected in ["accounts", "attendees", "calendars", "events",
                         "mutations", "settings", "sync_state"] {
            assert!(names.contains(&expected.to_string()), "missing table {expected}");
        }
    }

    #[tokio::test]
    async fn foreign_keys_are_enforced() {
        let pool = connect_memory().await.unwrap();
        let res = sqlx::query(
            "INSERT INTO calendars (account_id, google_id, summary, timezone, access_role)
             VALUES (999, 'x', 'x', 'UTC', 'owner')",
        )
        .execute(&pool)
        .await;
        assert!(res.is_err(), "insert with a dangling account_id should fail");
    }
}
