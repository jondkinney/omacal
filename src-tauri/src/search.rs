//! Search: the query half. The overlay is Task 2.
//!
//! Three layers, and the split is the same one `notify_loop` uses: the store
//! answers *which events match* (`omacal_store::search_events` — titles, and
//! only calendars the user displays), `omacal_core::search` answers *which
//! occurrence and in what order* against a clock it is handed, and this file
//! joins them.
//!
//! **No write path, no network** (spec §7). This is a `SELECT` and some
//! arithmetic. A search that synced first would be slow and surprising, and a
//! search that could change an event would be a second way to do what the
//! popover already does with every guard it has.

use omacal_core::search::{by_distance, nearest, Hit};
use omacal_core::layout::Interval;
use omacal_store::StoredEvent;
use sqlx::SqlitePool;

use crate::AppState;

/// How wide a net to cast when resolving a recurring match to one occurrence.
///
/// A year either side of the clock. It is a policy rather than a fact, and it
/// lives here — in the impure half — rather than in `omacal_core::search`,
/// which is handed occurrences and does not care where the window came from.
///
/// A year is enough for everything a person searches for by name: a weekly
/// standup, a monthly review, an annual trip all have an occurrence inside it.
/// What it misses is a series that stopped more than a year ago or starts more
/// than a year out, and [`resolve`]'s fallback is what covers those.
const NEAR_WINDOW_MS: i64 = 365 * 24 * 3_600_000;

/// How many occurrences the fallback expansion may produce before giving up.
///
/// Only the *far* path uses it, and only to bound a daily series expanded over
/// decades. `u16` because that is what `expand` takes.
const FAR_LIMIT: u16 = 2_000;

/// The occurrence of `src` nearest `now_ms`.
///
/// **Not the master's own start**, which for a series running since 2019 is
/// four years from anything anybody is looking for — and which is the answer a
/// naive implementation gives, because for a series beginning today the two
/// coincide and every simple test passes.
///
/// Two passes, and the second is what makes the first's window a convenience
/// rather than a correctness claim:
///
/// 1. A year either side. Covers everything a person searches by name.
/// 2. Failing that, from the series' own `DTSTART` to a year out. A series that
///    ended in 2021 resolves to its **last** occurrence; one that starts in
///    2030 resolves to its first. Bounded by [`FAR_LIMIT`], which for a daily
///    series is about five and a half years of it — enough to reach the end of
///    anything that has one.
///
/// Falls back to the row's own span only when both expansions come back empty,
/// which means a rule that generates nothing at all. Something is on screen for
/// the user either way; a result with no occurrence would be a row that cannot
/// be clicked.
fn resolve(src: &StoredEvent, now_ms: i64) -> Interval {
    let own = Interval { start_ms: src.start_utc, end_ms: src.end_utc };
    if src.recurrence.is_none() {
        return own;
    }

    let near = crate::commands::occurrences(src, now_ms - NEAR_WINDOW_MS, now_ms + NEAR_WINDOW_MS);
    if let Some(iv) = nearest(&near, now_ms) {
        return iv;
    }

    let far = crate::commands::occurrences_limited(
        src,
        src.start_utc,
        now_ms + NEAR_WINDOW_MS,
        FAR_LIMIT,
    );
    nearest(&far, now_ms).unwrap_or(own)
}

/// Events whose title contains `query`, nearest first.
///
/// `now_ms` is a parameter for the reason `due_reminders` takes one: "nearest
/// today" is the whole of the ordering, and a function that read a clock could
/// not be tested against a fixed one.
pub(crate) async fn search(
    pool: &SqlitePool,
    query: &str,
    now_ms: i64,
) -> anyhow::Result<Vec<Hit>> {
    // An empty query is not "match everything" — `LIKE '%%'` would return the
    // entire database, and the overlay asks on every keystroke including the
    // one that empties the field.
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }

    let rows = omacal_store::search_events(pool, query.trim()).await?;

    let mut hits: Vec<Hit> = rows
        .iter()
        .map(|src| {
            let iv = resolve(src, now_ms);
            Hit {
                event_id: src.id,
                title: src.summary.clone().unwrap_or_else(|| "(no title)".into()),
                start_ms: iv.start_ms,
                end_ms: iv.end_ms,
            }
        })
        .collect();

    by_distance(&mut hits, now_ms);
    Ok(hits)
}

#[tauri::command]
pub async fn search_events(
    state: tauri::State<'_, AppState>,
    query: String,
) -> Result<Vec<Hit>, String> {
    // The one clock read in this feature, and it is here rather than any
    // deeper: `search` takes `now_ms` so it can be driven against a fixed
    // one, exactly as `due_reminders` does.
    search(&state.pool, &query, crate::now_ms())
        .await
        .map_err(|e| crate::errors::user_facing(&e))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: i64 = 3_600_000;
    const DAY: i64 = 24 * HOUR;
    /// Monday 10 Aug 2026, 12:00 UTC. Fixed, because "nearest today" is the
    /// thing under test.
    const NOW: i64 = 1_786_017_600_000;

    async fn pool_with_two_calendars() -> SqlitePool {
        let pool = omacal_store::connect_memory().await.unwrap();
        sqlx::query("INSERT INTO accounts (google_sub, email, created_at) VALUES ('s','me@x.com',0)")
            .execute(&pool).await.unwrap();
        // 1 shown, 2 hidden. Both synced — `selected` and `sync_enabled` are
        // separate switches, and it is `selected` search follows.
        for (gid, name, selected) in [("shown", "Shown", 1), ("hidden", "Hidden", 0)] {
            sqlx::query(
                "INSERT INTO calendars
                     (account_id, google_id, summary, color_hex, timezone, access_role,
                      is_primary, selected, sync_enabled)
                 VALUES (1, ?1, ?2, '#5b8def', 'UTC', 'owner', 0, ?3, 1)")
                .bind(gid).bind(name).bind(selected)
                .execute(&pool).await.unwrap();
        }
        pool
    }

    fn row(cal: i64, gid: &str, title: &str, start_ms: i64) -> StoredEvent {
        StoredEvent {
            id: 0, calendar_id: cal, google_id: gid.into(), summary: Some(title.into()),
            location: None, start_utc: start_ms, end_utc: start_ms + HOUR,
            start_tz: "UTC".into(), end_tz: "UTC".into(), is_all_day: false,
            recurrence: None, recurring_event_id: None, original_start_utc: None,
            status: "confirmed".into(), self_response: None, conference_uri: None,
            color_hex: None, calendar_timezone: "UTC".into(),
            description: None, etag: None, sequence: 0, organizer_email: None,
            attendees: Vec::new(),
            reminders: Default::default(), calendar_default_reminders: Vec::new(),
        }
    }

    async fn insert(pool: &SqlitePool, e: &StoredEvent) {
        omacal_store::upsert_event(pool, e).await.unwrap();
    }

    #[tokio::test]
    async fn a_title_is_matched_case_insensitively_anywhere_in_it() {
        let pool = pool_with_two_calendars().await;
        insert(&pool, &row(1, "a", "Quarterly Board Review", NOW + DAY)).await;

        for q in ["board", "BOARD", "Board Review", "quarterly"] {
            let hits = search(&pool, q, NOW).await.unwrap();
            assert_eq!(hits.len(), 1, "query {q:?} should match");
        }
        assert!(search(&pool, "budget", NOW).await.unwrap().is_empty());
    }

    /// §2: titles and nothing else. A word in the location must not pull an
    /// event in, or results stop being explicable from what was typed.
    #[tokio::test]
    async fn a_match_in_the_location_is_not_a_match() {
        let pool = pool_with_two_calendars().await;
        let mut e = row(1, "a", "Standup", NOW + DAY);
        e.location = Some("Board room".into());
        insert(&pool, &e).await;

        assert!(search(&pool, "board", NOW).await.unwrap().is_empty());
    }

    /// **Spec §5, both halves in one query.** The absence alone would pass
    /// against a search that returns nothing at all, which is why the visible
    /// calendar's match is asserted in the same call.
    #[tokio::test]
    async fn a_hidden_calendars_events_are_not_found_while_a_shown_calendars_are() {
        let pool = pool_with_two_calendars().await;
        insert(&pool, &row(1, "shown-ev", "Team lunch", NOW + DAY)).await;
        insert(&pool, &row(2, "hidden-ev", "Team lunch", NOW + 2 * DAY)).await;

        let hits = search(&pool, "team lunch", NOW).await.unwrap();

        assert_eq!(hits.len(), 1, "the hidden calendar's copy must not appear");
        assert_eq!(hits[0].start_ms, NOW + DAY, "and the one that did is the shown one");
    }

    /// §4, with fixtures on **both sides of today** — without the past ones,
    /// nearest-first and soonest-first are the same order.
    #[tokio::test]
    async fn results_come_back_nearest_first_in_either_direction() {
        let pool = pool_with_two_calendars().await;
        insert(&pool, &row(1, "a", "Trip to Rome", NOW + 300 * DAY)).await;
        insert(&pool, &row(1, "b", "Trip to Lisbon", NOW - 2 * DAY)).await;
        insert(&pool, &row(1, "c", "Trip to Oslo", NOW + DAY)).await;
        insert(&pool, &row(1, "d", "Trip to Cairo", NOW - 400 * DAY)).await;

        let hits = search(&pool, "trip to", NOW).await.unwrap();

        assert_eq!(
            hits.iter().map(|h| h.title.as_str()).collect::<Vec<_>>(),
            vec!["Trip to Oslo", "Trip to Lisbon", "Trip to Rome", "Trip to Cairo"],
        );
    }

    /// **Spec §3, and the fixture is built to witness both mistakes.**
    ///
    /// A weekly standup running for two years is one row and about a hundred
    /// occurrences. One result — and the occurrence it resolves to is the
    /// nearest, which is neither the series' own `DTSTART` (two years ago) nor
    /// the first occurrence of any window.
    #[tokio::test]
    async fn a_long_running_series_is_one_result_at_its_nearest_occurrence() {
        let pool = pool_with_two_calendars().await;
        // DTSTART two years before the clock, on the same weekday, so the
        // occurrences land on NOW - 2y + 7n days.
        let dtstart = NOW - 728 * DAY;
        let mut e = row(1, "series", "Standup", dtstart);
        e.recurrence = Some("RRULE:FREQ=WEEKLY".into());
        insert(&pool, &e).await;

        let hits = search(&pool, "standup", NOW).await.unwrap();

        assert_eq!(hits.len(), 1, "a series is one result, not one per occurrence");
        assert_ne!(hits[0].start_ms, dtstart, "not the master's own start");
        // 728 is a multiple of 7, so an occurrence falls exactly on the clock.
        assert_eq!(hits[0].start_ms, NOW, "the nearest occurrence");
    }

    /// The other half of §3's rule: a series that **ended** long ago resolves
    /// to its last occurrence rather than its first. This is the case the
    /// near-window alone cannot answer, and the one where "the master's own
    /// start" is most obviously wrong.
    #[tokio::test]
    async fn a_series_that_ended_years_ago_resolves_to_its_last_occurrence() {
        let pool = pool_with_two_calendars().await;
        let dtstart = NOW - 1_400 * DAY;
        let mut e = row(1, "old", "Retro", dtstart);
        // Four weekly occurrences, then it stops.
        e.recurrence = Some("RRULE:FREQ=WEEKLY;COUNT=4".into());
        insert(&pool, &e).await;

        let hits = search(&pool, "retro", NOW).await.unwrap();

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].start_ms, dtstart + 21 * DAY, "the last of the four, not the first");
    }

    /// §2 again, from the direction a query language would break it: `%` is a
    /// character somebody typed, not a wildcard.
    ///
    /// **The fixture is built so an unescaped `%` gives a different answer**,
    /// which took a mutation to get right. A first version searched `80%`
    /// against "Q3 at 80% capacity" and a "Standup": unescaped that is
    /// `%80%%`, which still needs the literal "80" and so still matched one
    /// row — the test passed either way and witnessed nothing. Here the query
    /// spans the percent sign, so unescaped it matches the spelled-out title
    /// too and the count changes.
    #[tokio::test]
    async fn a_percent_sign_matches_a_percent_sign_and_not_anything() {
        let pool = pool_with_two_calendars().await;
        insert(&pool, &row(1, "a", "50% progress", NOW + DAY)).await;
        insert(&pool, &row(1, "b", "50 percent progress", NOW + 2 * DAY)).await;

        // `50% p` — a substring of the first title and, unescaped, a wildcard
        // match for the second as well ("50", anything, " p").
        let hits = search(&pool, "50% p", NOW).await.unwrap();

        assert_eq!(hits.len(), 1, "an unescaped % would match the spelled-out one too");
        assert_eq!(hits[0].title, "50% progress");
    }

    #[tokio::test]
    async fn an_underscore_is_a_character_too() {
        let pool = pool_with_two_calendars().await;
        insert(&pool, &row(1, "a", "sync_now", NOW + DAY)).await;
        insert(&pool, &row(1, "b", "syncXnow", NOW + 2 * DAY)).await;

        let hits = search(&pool, "sync_now", NOW).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "sync_now");
    }

    /// The overlay asks on every keystroke, including the one that empties the
    /// field. `LIKE '%%'` would answer with the whole database.
    #[tokio::test]
    async fn an_empty_query_matches_nothing_rather_than_everything() {
        let pool = pool_with_two_calendars().await;
        insert(&pool, &row(1, "a", "Standup", NOW + DAY)).await;

        assert!(search(&pool, "", NOW).await.unwrap().is_empty());
        assert!(search(&pool, "   ", NOW).await.unwrap().is_empty());
    }
}
