//! The scheduler driver — **the only thing in this feature that reads a clock.**
//!
//! Every rule about *which* reminders fire lives in `omacal_core::remind`,
//! where `now` is a parameter and no test has to sleep to observe anything.
//! What is left here is the part that genuinely cannot be pure: read the world,
//! call that function, post what is ready, record it, forget what is dead, and
//! work out when to wake. A rule that creeps into this file becomes untestable
//! without waiting, which is the whole reason the split exists.
//!
//! Note the shape of [`run_once`]: `now_ms` is still a parameter *here* too, so
//! the driver's own behaviour — what it posts, what it records, what it prunes,
//! when it would next wake — is tested at a fixed clock. Only [`spawn`] reads
//! the real one, and it is deliberately too thin to be wrong.

use omacal_core::remind::{due_reminders, Due, FiredKey, Reminder, ScheduledEvent};
use sqlx::SqlitePool;

/// How far ahead the scheduler looks (spec §4). Comfortably longer than the
/// sync interval, so a reminder cannot fall between two recomputations, and
/// short enough that expanding recurring series stays cheap.
pub(crate) const HORIZON_MS: i64 = 48 * 3_600_000;

/// The longest lead time Google accepts on a reminder: 40320 minutes, four
/// weeks.
///
/// The fetch window has to reach this far past the horizon, because the horizon
/// bounds *fire times* and a fire time is up to this long before the occurrence
/// it belongs to. An event four weeks out with a four-week reminder is due
/// right now, and a window that stopped at the horizon would not see it.
const MAX_REMINDER_LEAD_MS: i64 = 40_320 * 60_000;

/// One pass's worth of outcome, so a test can see everything the pass decided
/// without watching a clock.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Pass {
    /// Posted on this pass, and recorded.
    pub posted: Vec<Due>,
    /// The earliest fire-time still ahead of `now_ms`, if any. What [`spawn`]
    /// sleeps until.
    pub next_fire_ms: Option<i64>,
    /// Rows forgotten because their occurrence has ended.
    pub pruned: u64,
}

/// Maps one stored reminder into the shape the pure function takes.
fn to_core(r: &omacal_store::Reminder) -> Reminder {
    Reminder { method: r.method.clone(), minutes: r.minutes }
}

/// The scheduler's view of the world at `now_ms`: every occurrence that could
/// carry a reminder worth posting, expanded and carrying its calendar's zone.
///
/// **Only calendars with `selected = true` are represented, and that is
/// enforced by `events_in_window`'s own `WHERE c.selected = 1`** — the same
/// query the grid draws from. Deliberately the same one: §2.2 says a calendar
/// not worth drawing is not worth interrupting you, and reading notifications
/// off a second, parallel query is how the two would come to disagree.
/// `calendar_selected` below is therefore always `true` here; the field is the
/// pure function's own statement of the rule, and it is what a caller
/// constructing `ScheduledEvent`s by hand has to get right.
///
/// Cancelled rows and suppressed slots are skipped exactly as the four grid
/// assemblers skip them — a deleted occurrence of a series must not notify any
/// more than it should draw.
async fn scheduled_events(
    pool: &SqlitePool,
    now_ms: i64,
    horizon_ms: i64,
) -> anyhow::Result<Vec<ScheduledEvent>> {
    let to_ms = now_ms.saturating_add(horizon_ms).saturating_add(MAX_REMINDER_LEAD_MS);
    // From `now_ms`, not earlier: an occurrence that began in the past is still
    // returned while it is running, since both the store's window query and
    // `occurrences` keep anything whose end is still ahead — which is exactly
    // the set the missed-reminder rule needs and no more.
    let stored = omacal_store::events_in_window(pool, now_ms, to_ms).await?;
    let suppressed = crate::commands::suppressed_slots(&stored);

    let mut out = Vec::new();
    for src in &stored {
        // A cancelled exception exists only to record that an occurrence was
        // deleted. It has been counted into `suppressed`; it notifies nothing.
        if src.status == "cancelled" {
            continue;
        }
        for iv in crate::commands::occurrences(src, now_ms, to_ms) {
            if suppressed.contains(&(src.calendar_id, src.google_id.as_str(), iv.start_ms)) {
                continue;
            }
            out.push(ScheduledEvent {
                event_id: src.id,
                calendar_selected: true,
                calendar_tz: src.calendar_timezone.clone(),
                occurrence_start_ms: iv.start_ms,
                occurrence_end_ms: iv.end_ms,
                is_all_day: src.is_all_day,
                use_default_reminders: src.reminders.use_default,
                overrides: src.reminders.overrides.iter().map(to_core).collect(),
                calendar_defaults: src.calendar_default_reminders.iter().map(to_core).collect(),
            });
        }
    }
    Ok(out)
}

/// One pass of the scheduler, at a clock the caller supplies.
///
/// `post` is the seam the transport plugs into. It is a callback rather than a
/// notifier because the transport is not built yet; what matters for this task
/// is that everything around it — which reminders are ready, what gets
/// recorded, what gets forgotten — is decided here and tested without a clock.
///
/// **Ready means `fire_at_ms <= now_ms`, not "in the returned list".**
/// `due_reminders` hands back the whole schedule out to the horizon, most of
/// which is still in the future; posting all of it would fire two days of
/// reminders at once. Everything past that cut is what the driver sleeps until.
///
/// A reminder whose time has already gone by is posted immediately, which is
/// the missed rule (§2.3) arriving here: `fire_at_ms` may be well behind
/// `now_ms`, and the comparison above treats that as ready rather than as a
/// negative delay to wait out.
///
/// Recording follows posting, never precedes it. A crash in between re-posts
/// one reminder on the next pass; the other order silently swallows it forever.
pub(crate) async fn run_once(
    pool: &SqlitePool,
    now_ms: i64,
    horizon_ms: i64,
    post: &mut (dyn FnMut(&Due) + Send),
) -> anyhow::Result<Pass> {
    let events = scheduled_events(pool, now_ms, horizon_ms).await?;

    let fired: std::collections::HashSet<FiredKey> = omacal_store::fired_keys(pool)
        .await?
        .into_iter()
        .map(|(event_id, occurrence_ms, minutes)| FiredKey { event_id, occurrence_ms, minutes })
        .collect();

    let due = due_reminders(&events, &fired, now_ms, horizon_ms);

    let mut pass = Pass::default();
    for d in due {
        if d.fire_at_ms > now_ms {
            // Still ahead. `due_reminders` returns its results in fire-time
            // order, so the first one past the cut is the earliest.
            pass.next_fire_ms = Some(pass.next_fire_ms.map_or(d.fire_at_ms, |n: i64| n.min(d.fire_at_ms)));
            continue;
        }

        post(&d);

        // `d.occurrence_end_ms` rather than a lookup: the `Due` carries the end
        // of its own occurrence, so there is no search to come up empty and no
        // fallback to be wrong. Searching `events` would have to match the
        // anchor against a raw start, which for an all-day occurrence on a
        // moved calendar zone are different instants.
        omacal_store::record_fired(
            pool,
            d.key.event_id,
            d.key.occurrence_ms,
            d.key.minutes,
            d.occurrence_end_ms,
            now_ms,
        )
        .await?;

        pass.posted.push(d);
    }

    pass.pruned = omacal_store::prune_fired(pool, now_ms).await?;
    Ok(pass)
}

/// How long to sleep: until the next fire-time or the next sync, whichever is
/// sooner, and never a negative duration.
///
/// Clamped at zero because `next_fire_ms` can be behind `now_ms` in principle —
/// a pass that failed to post, a clock moved backwards — and a driver that
/// subtracted and slept would wait for hours or panic converting to a
/// `Duration`. Zero means "go round again now", which re-posts nothing, because
/// the record written on the last pass excludes it.
pub(crate) fn next_wake_ms(next_fire_ms: Option<i64>, now_ms: i64, sync_interval_ms: i64) -> i64 {
    let until_sync = sync_interval_ms.max(0);
    match next_fire_ms {
        Some(fire) => (fire - now_ms).clamp(0, until_sync),
        None => until_sync,
    }
}

/// The loop, and the only clock read in the feature.
///
/// Deliberately thin to the point of having nothing in it worth testing: every
/// decision it makes was made by [`run_once`] and [`next_wake_ms`], both of
/// which take their clock as an argument.
///
/// **Not wired into `run()` yet, and that is on purpose.** There is no notifier
/// until Task 4, so a pass now would record every due reminder as fired while
/// posting nothing — and a recorded reminder is never offered again. Starting
/// this early would silently consume the first day of notifications. Task 5
/// wires it, after the demo guard exists.
#[allow(dead_code)]
pub(crate) fn spawn(app: tauri::AppHandle, mut post: Box<dyn FnMut(&Due) + Send>) {
    tauri::async_runtime::spawn(async move {
        loop {
            let (pool, interval) = {
                let state = tauri::Manager::state::<crate::AppState>(&app);
                (state.pool.clone(), crate::sync_loop::interval_ms(&state.pool).await)
            };

            let now = crate::now_ms();
            let next_fire = match run_once(&pool, now, HORIZON_MS, &mut post).await {
                Ok(pass) => pass.next_fire_ms,
                Err(e) => {
                    // Offline, a locked database, a malformed rule — all normal
                    // and all transient. Never panic the app over one pass.
                    tracing::warn!(%e, "notification pass failed");
                    None
                }
            };

            let delay = next_wake_ms(next_fire, crate::now_ms(), interval);
            tokio::time::sleep(std::time::Duration::from_millis(delay as u64)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use omacal_store::{Reminders, StoredEvent};

    const MINUTE: i64 = 60_000;
    const HOUR: i64 = 60 * MINUTE;
    const DAY: i64 = 24 * HOUR;

    /// 2026-08-10T09:00:00Z.
    const T0900Z: i64 = 1_786_352_400_000;
    /// Midnight 2026-08-10 in `Europe/Sofia` — 2026-08-09T21:00Z.
    const SOFIA_MIDNIGHT_AUG10: i64 = 1_786_309_200_000;
    /// Midnight 2026-08-10 in `Pacific/Auckland` — 2026-08-09T12:00Z.
    const AUCKLAND_MIDNIGHT_AUG10: i64 = 1_786_276_800_000;

    async fn seeded(cal_tz: &str, defaults: &str) -> SqlitePool {
        let pool = omacal_store::connect_memory().await.unwrap();
        sqlx::query("INSERT INTO accounts (google_sub, email, created_at) VALUES ('s','e@x',0)")
            .execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO calendars
                 (account_id, google_id, summary, timezone, access_role, default_reminders_json)
             VALUES (1, 'primary', 'Work', ?1, 'owner', ?2)",
        )
        .bind(cal_tz)
        .bind(defaults)
        .execute(&pool).await.unwrap();
        pool
    }

    fn popup(minutes: i64) -> omacal_store::Reminder {
        omacal_store::Reminder { method: "popup".into(), minutes }
    }

    fn event(google_id: &str, start: i64, end: i64, reminders: Reminders) -> StoredEvent {
        StoredEvent {
            id: 0,
            calendar_id: 1,
            google_id: google_id.into(),
            summary: Some("Standup".into()),
            location: None,
            start_utc: start,
            end_utc: end,
            start_tz: "UTC".into(),
            end_tz: "UTC".into(),
            is_all_day: false,
            recurrence: None,
            recurring_event_id: None,
            original_start_utc: None,
            status: "confirmed".into(),
            self_response: None,
            conference_uri: None,
            color_hex: None,
            calendar_timezone: "UTC".into(),
            description: None,
            etag: None,
            sequence: 0,
            organizer_email: None,
            attendees: Vec::new(),
            reminders,
            calendar_default_reminders: Vec::new(),
        }
    }

    fn own(minutes: i64) -> Reminders {
        Reminders { use_default: false, overrides: vec![popup(minutes)] }
    }

    /// Collects what a pass would post, without any transport.
    async fn pass_at(pool: &SqlitePool, now_ms: i64) -> (Pass, Vec<(i64, i64)>) {
        let mut posted = Vec::new();
        let pass = {
            let mut sink = |d: &Due| posted.push((d.key.event_id, d.key.minutes));
            run_once(pool, now_ms, HORIZON_MS, &mut sink).await.unwrap()
        };
        (pass, posted)
    }

    #[tokio::test]
    async fn a_reminder_that_is_due_is_posted_and_recorded() {
        let pool = seeded("UTC", "[]").await;
        omacal_store::upsert_event(&pool, &event("e1", T0900Z, T0900Z + HOUR, own(10)))
            .await.unwrap();

        // Ten minutes before the meeting: exactly when the reminder is due.
        let (pass, posted) = pass_at(&pool, T0900Z - 10 * MINUTE).await;

        assert_eq!(posted, vec![(1, 10)]);
        assert_eq!(pass.posted.len(), 1);
        assert_eq!(
            omacal_store::fired_keys(&pool).await.unwrap(),
            vec![(1, T0900Z, 10)],
            "posting without recording re-posts on the next pass"
        );
    }

    /// A fire-time still ahead is scheduled, not posted. `due_reminders` hands
    /// back the whole 48-hour schedule, and posting all of it would fire two
    /// days of reminders at once.
    #[tokio::test]
    async fn a_reminder_still_in_the_future_is_scheduled_rather_than_posted() {
        let pool = seeded("UTC", "[]").await;
        omacal_store::upsert_event(&pool, &event("e1", T0900Z, T0900Z + HOUR, own(10)))
            .await.unwrap();

        let (pass, posted) = pass_at(&pool, T0900Z - 3 * HOUR).await;

        assert!(posted.is_empty(), "a reminder due in hours must not fire now");
        assert_eq!(pass.next_fire_ms, Some(T0900Z - 10 * MINUTE));
        assert!(
            omacal_store::fired_keys(&pool).await.unwrap().is_empty(),
            "nothing may be recorded that was not posted, or it never fires at all"
        );
    }

    /// **The restart case, and the reason the table exists.**
    ///
    /// The second pass rebuilds everything from the database — events, fired
    /// state, the lot — exactly as a fresh process would. The clock is
    /// deliberately still *inside* the meeting: past its end the missed rule
    /// would decline to fire for its own reasons and this would pass without
    /// witnessing the record at all.
    #[tokio::test]
    async fn a_recorded_reminder_is_not_posted_again_after_a_restart() {
        let pool = seeded("UTC", "[]").await;
        omacal_store::upsert_event(&pool, &event("e1", T0900Z, T0900Z + HOUR, own(10)))
            .await.unwrap();

        let (_, first) = pass_at(&pool, T0900Z - 10 * MINUTE).await;
        assert_eq!(first, vec![(1, 10)], "the first pass must actually post something");

        // Five minutes in: the meeting is running, so the missed rule would
        // gladly fire this again if nothing had been recorded.
        let now = T0900Z + 5 * MINUTE;
        assert!(now < T0900Z + HOUR, "fixture check: the meeting must still be running");

        let (_, second) = pass_at(&pool, now).await;
        assert!(second.is_empty(), "a restart mid-meeting must not re-post the reminder");
    }

    /// The other half of that: with the record cleared, the same clock *does*
    /// post. Without this, the test above would pass against a driver that
    /// simply stopped posting after the first pass.
    #[tokio::test]
    async fn the_same_clock_posts_again_once_the_record_is_gone() {
        let pool = seeded("UTC", "[]").await;
        omacal_store::upsert_event(&pool, &event("e1", T0900Z, T0900Z + HOUR, own(10)))
            .await.unwrap();

        pass_at(&pool, T0900Z - 10 * MINUTE).await;
        sqlx::query("DELETE FROM fired_reminders").execute(&pool).await.unwrap();

        let (_, again) = pass_at(&pool, T0900Z + 5 * MINUTE).await;
        assert_eq!(again, vec![(1, 10)], "the record is the only thing suppressing this");
    }

    /// §2.3: a reminder missed while omacal was not running fires at launch,
    /// while the meeting is still on.
    #[tokio::test]
    async fn a_reminder_missed_while_the_app_was_shut_fires_at_the_next_pass() {
        let pool = seeded("UTC", "[]").await;
        omacal_store::upsert_event(&pool, &event("e1", T0900Z, T0900Z + HOUR, own(10)))
            .await.unwrap();

        // First run of the day is five minutes into the meeting.
        let (pass, posted) = pass_at(&pool, T0900Z + 5 * MINUTE).await;

        assert_eq!(posted, vec![(1, 10)]);
        assert!(
            pass.posted[0].fire_at_ms < T0900Z + 5 * MINUTE,
            "its fire time is in the past, which is the whole point of the rule"
        );
    }

    /// §2.2, end to end: a hidden calendar interrupts nobody. Enforced by
    /// `events_in_window`'s own filter, which is why this is asserted here and
    /// not only as a rule in the pure function.
    #[tokio::test]
    async fn a_hidden_calendars_events_notify_nothing() {
        let pool = seeded("UTC", "[]").await;
        omacal_store::upsert_event(&pool, &event("e1", T0900Z, T0900Z + HOUR, own(10)))
            .await.unwrap();

        let (_, shown) = pass_at(&pool, T0900Z - 10 * MINUTE).await;
        assert_eq!(shown, vec![(1, 10)], "fixture check: it fires while the calendar is shown");

        sqlx::query("DELETE FROM fired_reminders").execute(&pool).await.unwrap();
        sqlx::query("UPDATE calendars SET selected = 0").execute(&pool).await.unwrap();

        let (_, hidden) = pass_at(&pool, T0900Z - 10 * MINUTE).await;
        assert!(hidden.is_empty(), "deselecting a calendar cancels its pending reminders");
    }

    /// A deleted occurrence of a series must not notify, any more than it
    /// should draw. The cancelled exception is what records the deletion.
    #[tokio::test]
    async fn a_deleted_occurrence_of_a_series_notifies_nothing() {
        let pool = seeded("UTC", "[]").await;

        let mut master = event("m1", T0900Z, T0900Z + HOUR, own(10));
        master.recurrence = Some("RRULE:FREQ=DAILY".into());
        omacal_store::upsert_event(&pool, &master).await.unwrap();

        // Tomorrow's occurrence is deleted.
        let mut gone = event("m1_20260811", T0900Z + DAY, T0900Z + DAY + HOUR, own(10));
        gone.status = "cancelled".into();
        gone.recurring_event_id = Some("m1".into());
        gone.original_start_utc = Some(T0900Z + DAY);
        omacal_store::upsert_event(&pool, &gone).await.unwrap();

        // Ten minutes before the *deleted* occurrence would have started.
        let (_, posted) = pass_at(&pool, T0900Z + DAY - 10 * MINUTE).await;
        assert!(posted.is_empty(), "a cancelled occurrence must not notify");
    }

    /// The all-day case, keyed by the anchor rather than the raw start, and the
    /// fixture is the divergence: the event was stored as midnight in
    /// `Europe/Sofia` while its calendar now says `Pacific/Auckland`. Recording
    /// the raw start would write a key that the next pass fails to match, and
    /// every all-day reminder would re-post after every restart.
    #[tokio::test]
    async fn an_all_day_reminder_is_recorded_under_the_anchor_the_pure_function_computes() {
        let pool = seeded("Pacific/Auckland", "[]").await;

        let mut ev = event("d1", SOFIA_MIDNIGHT_AUG10, SOFIA_MIDNIGHT_AUG10 + DAY, own(30));
        ev.is_all_day = true;
        ev.start_tz = "Europe/Sofia".into();
        omacal_store::upsert_event(&pool, &ev).await.unwrap();

        assert_ne!(
            SOFIA_MIDNIGHT_AUG10, AUCKLAND_MIDNIGHT_AUG10,
            "fixture check: the stored start must not already be the anchor"
        );

        let (_, posted) = pass_at(&pool, AUCKLAND_MIDNIGHT_AUG10 - 30 * MINUTE).await;
        assert_eq!(posted, vec![(1, 30)]);
        assert_eq!(
            omacal_store::fired_keys(&pool).await.unwrap(),
            vec![(1, AUCKLAND_MIDNIGHT_AUG10, 30)],
            "the recorded key must be the anchor, not the instant the store held"
        );

        // The recorded *end* is the occurrence's own, not one derived from the
        // anchor. Asserted directly, because its only visible effect is when
        // the row is pruned — and an end that is wrong by the offset between
        // the two zones prunes the row early and re-posts the reminder, which
        // takes three passes to show up. This states it in one.
        let end_ms: i64 = sqlx::query_scalar("SELECT occurrence_end_ms FROM fired_reminders")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(
            end_ms,
            SOFIA_MIDNIGHT_AUG10 + DAY,
            "the occurrence's real end must be recorded, or the row is pruned early"
        );

        // And the record must actually suppress it on a rebuild.
        let (_, again) = pass_at(&pool, AUCKLAND_MIDNIGHT_AUG10 + HOUR).await;
        assert!(again.is_empty(), "an all-day reminder re-posted after a restart");

        // A third pass, and it is the one that matters. `run_once` prunes
        // *after* it consults the record, so a row pruned wrongly on pass two
        // is still present when pass two needs it — the damage only surfaces on
        // the pass after that. Two passes stopped one short of the bug.
        let (_, third) = pass_at(&pool, AUCKLAND_MIDNIGHT_AUG10 + 2 * HOUR).await;
        assert!(third.is_empty(), "the record was pruned early and the reminder came back");
    }

    /// The row for a long occurrence survives while it is still running.
    /// Pruned on the anchor plus a horizon, a week-long trip loses its record
    /// on day three and the missed rule posts it all over again.
    #[tokio::test]
    async fn a_still_running_occurrence_keeps_its_record_past_the_horizon() {
        let pool = seeded("UTC", "[]").await;
        omacal_store::upsert_event(&pool, &event("trip", T0900Z, T0900Z + 7 * DAY, own(30)))
            .await.unwrap();

        let (_, posted) = pass_at(&pool, T0900Z - 30 * MINUTE).await;
        assert_eq!(posted, vec![(1, 30)]);

        // Three days in — well past the 48-hour horizon, still mid-trip.
        let (pass, later) = pass_at(&pool, T0900Z + 3 * DAY).await;
        assert_eq!(pass.pruned, 0, "the record must outlive the horizon while the event runs");
        assert!(later.is_empty(), "and so the reminder must not fire a second time");
    }

    #[tokio::test]
    async fn a_finished_occurrences_record_is_forgotten() {
        let pool = seeded("UTC", "[]").await;
        omacal_store::upsert_event(&pool, &event("e1", T0900Z, T0900Z + HOUR, own(10)))
            .await.unwrap();

        pass_at(&pool, T0900Z - 10 * MINUTE).await;
        let (pass, _) = pass_at(&pool, T0900Z + HOUR).await;

        assert_eq!(pass.pruned, 1);
        assert!(omacal_store::fired_keys(&pool).await.unwrap().is_empty());
    }

    /// The calendar's defaults reach the driver, so an event that defers fires
    /// what the calendar says. Without the join this is silently nothing.
    #[tokio::test]
    async fn an_event_deferring_to_its_calendar_fires_the_calendars_defaults() {
        let pool = seeded("UTC", r#"[{"method":"popup","minutes":45}]"#).await;
        let ev = event(
            "e1",
            T0900Z,
            T0900Z + HOUR,
            Reminders { use_default: true, overrides: Vec::new() },
        );
        omacal_store::upsert_event(&pool, &ev).await.unwrap();

        let (_, posted) = pass_at(&pool, T0900Z - 45 * MINUTE).await;
        assert_eq!(posted, vec![(1, 45)], "the calendar's default reminder did not reach here");
    }

    #[test]
    fn the_next_wake_is_the_sooner_of_the_next_reminder_and_the_next_sync() {
        let sync = 5 * MINUTE;
        assert_eq!(
            next_wake_ms(Some(T0900Z + MINUTE), T0900Z, sync),
            MINUTE,
            "a reminder before the next sync wins"
        );
        assert_eq!(
            next_wake_ms(Some(T0900Z + HOUR), T0900Z, sync),
            sync,
            "a reminder after the next sync must not delay the sync"
        );
        assert_eq!(next_wake_ms(None, T0900Z, sync), sync, "nothing scheduled: wait for the sync");
    }

    /// A fire-time already behind the clock must never become a negative sleep.
    #[test]
    fn a_fire_time_already_past_wakes_immediately_rather_than_sleeping_backwards() {
        assert_eq!(next_wake_ms(Some(T0900Z - HOUR), T0900Z, 5 * MINUTE), 0);
        assert_eq!(next_wake_ms(Some(T0900Z), T0900Z, 5 * MINUTE), 0);
    }
}
