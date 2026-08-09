//! What has already been posted.
//!
//! Three operations, and the driver uses all three on every pass: read what has
//! fired, record what it posts, forget what can never fire again.
//!
//! The key is `(event_id, occurrence_ms, minutes)`, matching
//! `omacal_core::remind::FiredKey` field for field. It is kept as three plain
//! `i64`s here rather than borrowing that type, so this crate stays free of
//! `omacal-core`; the driver does the mapping, by name, in one place.

use sqlx::SqlitePool;

/// Records that one reminder has been posted.
///
/// `occurrence_ms` must be the anchor `due_reminders` computed, and
/// `occurrence_end_ms` that occurrence's end — the second is what
/// [`prune_fired`] later reads.
///
/// Re-recording an existing key does nothing rather than failing. The driver
/// posts and then records, so a duplicate inside one pass is a bug elsewhere;
/// it must not take the notification loop down with a constraint violation.
/// `DO NOTHING` rather than an update, so `fired_at_ms` stays the instant the
/// reminder actually went out.
pub async fn record_fired(
    pool: &SqlitePool,
    event_id: i64,
    occurrence_ms: i64,
    minutes: i64,
    occurrence_end_ms: i64,
    fired_at_ms: i64,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO fired_reminders
             (event_id, occurrence_ms, minutes, occurrence_end_ms, fired_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (event_id, occurrence_ms, minutes) DO NOTHING",
    )
    .bind(event_id)
    .bind(occurrence_ms)
    .bind(minutes)
    .bind(occurrence_end_ms)
    .bind(fired_at_ms)
    .execute(pool)
    .await?;
    Ok(())
}

/// Every recorded key, as `(event_id, occurrence_ms, minutes)`.
///
/// The whole table, unfiltered, and that is affordable only because
/// [`prune_fired`] runs on the same pass: the row count is bounded by how many
/// reminders are attached to occurrences that have not yet ended.
pub async fn fired_keys(pool: &SqlitePool) -> anyhow::Result<Vec<(i64, i64, i64)>> {
    Ok(sqlx::query_as(
        "SELECT event_id, occurrence_ms, minutes FROM fired_reminders
         ORDER BY event_id, occurrence_ms, minutes",
    )
    .fetch_all(pool)
    .await?)
}

/// Forgets every reminder whose occurrence has ended, returning how many went.
///
/// `occurrence_end_ms <= now_ms`, matching `due_reminders`' own cutoff exactly:
/// it fires a missed reminder only while `now < occurrence_end_ms`, so at
/// `now == end` the reminder can never be produced again and the row is dead.
/// A stricter comparison would keep rows forever; a looser one would drop a row
/// while its reminder could still come back, and it would be posted twice.
pub async fn prune_fired(pool: &SqlitePool, now_ms: i64) -> anyhow::Result<u64> {
    Ok(sqlx::query("DELETE FROM fired_reminders WHERE occurrence_end_ms <= ?1")
        .bind(now_ms)
        .execute(pool)
        .await?
        .rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connect_memory;
    use sqlx::SqlitePool;

    const HOUR: i64 = 3_600_000;
    const DAY: i64 = 24 * HOUR;

    /// 2026-08-10T09:00:00Z, the occurrence anchor these tests key on.
    const T0900Z: i64 = 1_786_352_400_000;

    async fn seed(pool: &SqlitePool) {
        sqlx::query("INSERT INTO accounts (google_sub, email, created_at) VALUES ('s','e@x',0)")
            .execute(pool).await.unwrap();
        sqlx::query(
            "INSERT INTO calendars (account_id, google_id, summary, timezone, access_role)
             VALUES (1, 'primary', 'Work', 'Europe/Sofia', 'owner')",
        )
        .execute(pool).await.unwrap();
    }

    /// Every field distinct, and distinct from every other field's value, so a
    /// transposed bind — `minutes` written into `occurrence_ms`, say — cannot
    /// round-trip by coincidence. Three bare `i64`s in a row is exactly the
    /// shape that transposes silently.
    #[tokio::test]
    async fn a_recorded_reminder_round_trips_with_each_field_in_its_own_column() {
        let pool = connect_memory().await.unwrap();
        seed(&pool).await;

        record_fired(&pool, 7, T0900Z, 30, T0900Z + HOUR, T0900Z - 30 * 60_000)
            .await
            .unwrap();

        let (event_id, occurrence_ms, minutes, occurrence_end_ms, fired_at_ms): (
            i64, i64, i64, i64, i64,
        ) = sqlx::query_as(
            "SELECT event_id, occurrence_ms, minutes, occurrence_end_ms, fired_at_ms
             FROM fired_reminders",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(event_id, 7);
        assert_eq!(occurrence_ms, T0900Z);
        assert_eq!(minutes, 30);
        assert_eq!(occurrence_end_ms, T0900Z + HOUR);
        assert_eq!(fired_at_ms, T0900Z - 30 * 60_000);

        assert_eq!(
            fired_keys(&pool).await.unwrap(),
            vec![(7, T0900Z, 30)],
            "the key read back must be (event_id, occurrence_ms, minutes) in that order"
        );
    }

    /// The whole reason the table exists. A reminder recorded once must not be
    /// offered again — this is the restart-during-a-meeting case, and it is
    /// `fired_keys` that has to report it.
    #[tokio::test]
    async fn a_recorded_reminder_is_reported_as_already_fired() {
        let pool = connect_memory().await.unwrap();
        seed(&pool).await;

        record_fired(&pool, 1, T0900Z, 10, T0900Z + HOUR, T0900Z).await.unwrap();
        assert!(fired_keys(&pool).await.unwrap().contains(&(1, T0900Z, 10)));
    }

    /// An event's 10-minute and 1-minute reminders are separate rows. Dropping
    /// `minutes` from the primary key collapses them, and the second reminder
    /// is silently never posted.
    #[tokio::test]
    async fn an_events_two_reminders_are_recorded_separately() {
        let pool = connect_memory().await.unwrap();
        seed(&pool).await;

        record_fired(&pool, 1, T0900Z, 10, T0900Z + HOUR, T0900Z).await.unwrap();
        record_fired(&pool, 1, T0900Z, 1, T0900Z + HOUR, T0900Z).await.unwrap();

        let mut keys = fired_keys(&pool).await.unwrap();
        keys.sort();
        assert_eq!(
            keys,
            vec![(1, T0900Z, 1), (1, T0900Z, 10)],
            "both reminders of one occurrence must survive as separate rows"
        );
    }

    /// A weekly standup fires every week. Dropping `occurrence_ms` from the key
    /// collapses every week into one row and the series notifies once, ever.
    #[tokio::test]
    async fn two_occurrences_of_one_event_are_recorded_separately() {
        let pool = connect_memory().await.unwrap();
        seed(&pool).await;

        record_fired(&pool, 1, T0900Z, 10, T0900Z + HOUR, T0900Z).await.unwrap();
        record_fired(&pool, 1, T0900Z + 7 * DAY, 10, T0900Z + 7 * DAY + HOUR, T0900Z)
            .await
            .unwrap();

        assert_eq!(fired_keys(&pool).await.unwrap().len(), 2);
    }

    /// Recording the same reminder twice must not error. The driver posts and
    /// then records; a duplicate in one pass is a bug elsewhere, but it must
    /// not take the notification loop down with a constraint violation.
    #[tokio::test]
    async fn recording_the_same_reminder_twice_is_not_an_error() {
        let pool = connect_memory().await.unwrap();
        seed(&pool).await;

        record_fired(&pool, 1, T0900Z, 10, T0900Z + HOUR, T0900Z).await.unwrap();
        record_fired(&pool, 1, T0900Z, 10, T0900Z + HOUR, T0900Z + 5)
            .await
            .expect("a second record of the same key must be a no-op, not a failure");

        assert_eq!(fired_keys(&pool).await.unwrap().len(), 1);
        let fired_at: i64 = sqlx::query_scalar("SELECT fired_at_ms FROM fired_reminders")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(fired_at, T0900Z, "the first posting is the record; it is not rewritten");
    }

    /// Pruning is keyed on when the occurrence **ended**, not on when it
    /// started, and the difference is a re-fire.
    ///
    /// The long row here is the case: a week-long trip whose reminder fired
    /// before it began. Pruned on start-plus-a-horizon it would be gone by day
    /// three — and `due_reminders`' missed rule fires anything whose time has
    /// passed while `now < occurrence_end`, so the trip would notify again.
    #[tokio::test]
    async fn pruning_keeps_a_row_until_its_occurrence_has_actually_ended() {
        let pool = connect_memory().await.unwrap();
        seed(&pool).await;

        // A week-long occurrence that began three days ago and is still on.
        record_fired(&pool, 1, T0900Z - 3 * DAY, 30, T0900Z + 4 * DAY, T0900Z - 3 * DAY)
            .await
            .unwrap();
        // An hour-long meeting that ended yesterday.
        record_fired(&pool, 2, T0900Z - DAY, 10, T0900Z - DAY + HOUR, T0900Z - DAY)
            .await
            .unwrap();
        // One still to come.
        record_fired(&pool, 3, T0900Z + DAY, 10, T0900Z + DAY + HOUR, T0900Z)
            .await
            .unwrap();

        let removed = prune_fired(&pool, T0900Z).await.unwrap();
        assert_eq!(removed, 1, "only the meeting that is over may be forgotten");

        let mut left: Vec<i64> = fired_keys(&pool).await.unwrap().iter().map(|k| k.0).collect();
        left.sort();
        assert_eq!(
            left,
            vec![1, 3],
            "the still-running occurrence must keep its row, or its reminder fires again"
        );
    }

    /// The boundary: an occurrence is over *at* its end instant, since
    /// `due_reminders` fires only while `now < occurrence_end_ms`. A row whose
    /// occurrence ends exactly now can never produce another reminder.
    #[tokio::test]
    async fn a_row_is_prunable_exactly_when_its_occurrence_can_no_longer_fire() {
        let pool = connect_memory().await.unwrap();
        seed(&pool).await;

        record_fired(&pool, 1, T0900Z - HOUR, 10, T0900Z, T0900Z - HOUR).await.unwrap();
        assert_eq!(prune_fired(&pool, T0900Z - 1).await.unwrap(), 0, "one ms left to run");
        assert_eq!(prune_fired(&pool, T0900Z).await.unwrap(), 1, "over at exactly its end");
    }
}
