//! Which reminders are due, as a function of its arguments.
//!
//! **Nothing here reads a clock.** `now_ms` is a parameter, which is what makes
//! every rule below testable without a test sleeping to observe it. A
//! scheduler that reads the time inside itself can only be tested by waiting,
//! and this project has found repeatedly that such tests either run slowly or
//! assert nothing. If something here wants the current time, this signature is
//! wrong — not the test.
//!
//! The driver is the only thing that sleeps: it calls [`due_reminders`], posts
//! what comes back, records it, and waits for the earlier of the next
//! fire-time or the next sync.

use crate::zone::midnight_of_instant_in_zone;
use std::collections::HashSet;

/// The only `method` this app posts itself.
///
/// Google delivers the `email` ones from its own servers, so firing those here
/// would double every one. Google's own vocabulary, matched verbatim.
const POPUP: &str = "popup";

/// One reminder Google holds for an event: fire `minutes` before it starts, by
/// `method`.
///
/// A third shape of the same two fields, after `omacal_google::model::Reminder`
/// and `omacal_store::Reminder`, and deliberately so: this crate is pure and
/// depends on neither `sqlx` nor `reqwest`, which is the property that lets the
/// rules below be tested without a database or a server. The driver maps the
/// stored shape into this one, the same way sync maps the wire shape into the
/// stored one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reminder {
    /// `popup` or `email`. Only [`POPUP`] fires locally; see [`due_reminders`].
    pub method: String,
    pub minutes: i64,
}

/// One occurrence of one event, as the scheduler sees it: already expanded out
/// of its recurrence, and already carrying the two things that live on its
/// calendar rather than on itself.
///
/// `calendar_selected` and `calendar_tz` are copied onto each occurrence rather
/// than passed as a separate calendar list, so this function cannot be called
/// with an event whose calendar is missing — there is no lookup here to fail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledEvent {
    /// The `events` row id. Half of the identity of a fired reminder, and the
    /// reason a `Due` needs no other way back to its event.
    pub event_id: i64,
    /// Whether the owning calendar is **drawn**, not whether it is synced.
    /// §2.2: if it is not worth drawing, it is not worth interrupting you.
    pub calendar_selected: bool,
    /// The owning calendar's own IANA zone — never the display zone. What an
    /// all-day occurrence's midnight is read in.
    pub calendar_tz: String,
    pub occurrence_start_ms: i64,
    /// Exclusive, as everywhere else in this codebase. An occurrence is over at
    /// this instant, not still running through it.
    pub occurrence_end_ms: i64,
    pub is_all_day: bool,
    /// Google's `reminders.useDefault`. When true the calendar's list applies
    /// and `overrides` is ignored; the two are alternatives, never additive.
    pub use_default_reminders: bool,
    pub overrides: Vec<Reminder>,
    /// The owning calendar's `defaultReminders`.
    pub calendar_defaults: Vec<Reminder>,
    /// The event's summary. `None` for an event Google holds with no title.
    ///
    /// This and the two below are carried, never read: nothing here formats or
    /// interprets them. They are here so a [`Due`] can be self-sufficient —
    /// see that type for why a driver must not have to look its event back up.
    pub title: Option<String>,
    pub location: Option<String>,
    /// The occurrence's conferencing link, when it has one. Whether a *Join*
    /// action appears is decided by whether this is `Some`.
    pub conference_uri: Option<String>,
}

/// What identifies one posted reminder, and exactly the primary key of the
/// `fired_reminders` table the driver records into.
///
/// All three parts are load-bearing. Without `occurrence_ms` a weekly standup
/// fires once and never again; without `minutes` an event's 10-minute reminder
/// swallows its 1-minute one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FiredKey {
    pub event_id: i64,
    /// The occurrence's start **as [`due_reminders`] computes it** — for an
    /// all-day occurrence that is midnight in the calendar's zone, not whatever
    /// instant the expansion produced. Recomputed identically on every pass, so
    /// a recorded row still matches after a restart.
    pub occurrence_ms: i64,
    pub minutes: i64,
}

/// One reminder that should be posted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Due {
    pub key: FiredKey,
    /// When this reminder was due. **May be in the past** — see the missed rule
    /// in [`due_reminders`] — so a driver must treat a negative delay as "now"
    /// rather than sleeping on it.
    pub fire_at_ms: i64,
    /// The end of the occurrence this reminder belongs to, carried straight
    /// from its [`ScheduledEvent`].
    ///
    /// Here rather than looked up by the caller because it is the instant after
    /// which this reminder can never be produced again — rule 7's own cutoff —
    /// and so the only correct thing to record alongside it and prune on.
    ///
    /// A driver that instead searched its input for the matching event would
    /// have to match on [`FiredKey::occurrence_ms`], which is the *anchor*
    /// while `ScheduledEvent` holds the raw start. For an all-day occurrence on
    /// a calendar whose zone has moved those differ, the search silently finds
    /// nothing, and the row is recorded with whatever fallback the caller
    /// invented — pruned early, then posted a second time. Carrying the value
    /// deletes that whole class of mistake rather than documenting it.
    pub occurrence_end_ms: i64,
    /// Copied from the [`ScheduledEvent`] this came from, for the same reason
    /// as `occurrence_end_ms` above: **a `Due` carries everything needed to
    /// post and record it**, so nothing downstream has to search its way back
    /// to the event.
    ///
    /// That search is not hypothetical. It would have to match
    /// [`FiredKey::occurrence_ms`] — an anchor — against
    /// `ScheduledEvent::occurrence_start_ms` — a raw start — and for an all-day
    /// occurrence on a calendar whose zone has moved those differ. Returning an
    /// index into the caller's slice instead would be worse still: a caller
    /// that filtered or reordered its events between the call and the use would
    /// silently post a different meeting's name.
    pub title: Option<String>,
    pub location: Option<String>,
    pub conference_uri: Option<String>,
    /// Whether this occurrence is all-day, so a caller can say "All day"
    /// rather than rendering the anchor as a time.
    pub is_all_day: bool,
}

/// Every reminder that should be posted, given the world at `now_ms`.
///
/// Pure: no clock, no I/O, no database. The rules, in the order they are
/// applied:
///
/// 1. **Only calendars with `selected = true`.** §2.2.
/// 2. **An all-day occurrence is anchored to midnight in the calendar's own
///    zone**, never the display zone and never UTC. §2.1.
/// 3. **`use_default_reminders` chooses the list**: the calendar's defaults, or
///    the event's own overrides — one or the other, never merged. And when the
///    chosen defaults are **empty** and the occurrence is **timed**, the
///    caller's `fallback` applies instead (fallback spec §1): a receiving-only
///    account sees no reminders on a shared calendar at all, and every meeting
///    on it was silent. A fallback, never a merge — real overrides and real
///    defaults are untouched — and never for an all-day occurrence, whose
///    anchor is midnight and whose "60 minutes before" would knock at 23:00
///    the night before every trip.
/// 4. **Only `method == "popup"` fires.** Google sends the `email` ones itself.
/// 5. **Already-recorded reminders are excluded**, keyed by
///    [`FiredKey`], which is what stops a restart re-posting everything.
/// 6. **A future fire-time must fall inside `[now_ms, now_ms + horizon_ms)`.**
///    Measured against the fire time, not the occurrence start: an event
///    beginning just past the horizon with a long lead time has a reminder
///    inside it. Half-open, matching `events_in_window`.
/// 7. **A fire-time already past still fires, but only while
///    `now_ms < occurrence_end_ms`.** §2.3: you hear about a meeting that is
///    starting or in progress; one already over is dropped.
///
/// Rules 6 and 7 are deliberately separate branches on separate conditions.
/// They are easy to collapse into a single range check, and then neither is
/// provable on its own.
///
/// Returned sorted by `fire_at_ms`, earliest first, ties broken by the key, so
/// the driver can take the head and so the result does not depend on input
/// order.
pub fn due_reminders(
    events: &[ScheduledEvent],
    fired: &HashSet<FiredKey>,
    now_ms: i64,
    horizon_ms: i64,
    fallback: &[Reminder],
) -> Vec<Due> {
    let mut out: Vec<Due> = Vec::new();

    for ev in events {
        // Rule 1.
        if !ev.calendar_selected {
            continue;
        }

        // Rule 2, and it does real work rather than restating an invariant.
        //
        // An expanded all-day occurrence arrives as midnight in the *event's*
        // `start_tz` — that is the zone `commands::occurrences` hands to
        // `expand`, and the zone `recur` re-anchors each occurrence in. What
        // §2.1 asks for is midnight in the **calendar's** zone, and the store
        // keeps those as two separate columns precisely because they diverge:
        // a calendar whose zone was changed in Google's settings still has
        // events stamped with the zone that was current when sync stored them.
        // When they differ this line is the only thing that reconciles them.
        //
        // The `else` is equally load-bearing in the other direction: a timed
        // occurrence keeps its own time of day, and anchoring it to midnight
        // would fire every 09:00 meeting's reminder at half past midnight.
        let anchor_ms = if ev.is_all_day {
            midnight_of_instant_in_zone(ev.occurrence_start_ms, &ev.calendar_tz)
        } else {
            ev.occurrence_start_ms
        };

        // Rule 3, both halves. The second asks about the *chosen* list, not
        // about the event: overrides that happen to be empty chose "no
        // reminders" on purpose (`use_default: false, overrides: []` is what
        // the form writes for exactly that), and the fallback must not
        // overrule a person who said none.
        let chosen =
            if ev.use_default_reminders { &ev.calendar_defaults } else { &ev.overrides };
        let reminders = if ev.use_default_reminders && chosen.is_empty() && !ev.is_all_day {
            fallback
        } else {
            chosen
        };

        for r in reminders {
            // Rule 4.
            if r.method != POPUP {
                continue;
            }

            let key = FiredKey {
                event_id: ev.event_id,
                occurrence_ms: anchor_ms,
                minutes: r.minutes,
            };

            // Rule 5.
            if fired.contains(&key) {
                continue;
            }

            let fire_at_ms = anchor_ms - r.minutes * 60_000;

            let should_fire = if fire_at_ms >= now_ms {
                // Rule 6: still ahead of us, so it has to be near enough to be
                // worth holding.
                fire_at_ms < now_ms + horizon_ms
            } else {
                // Rule 7: missed while omacal was not running. Fires only while
                // the occurrence it belongs to still matters.
                now_ms < ev.occurrence_end_ms
            };

            if should_fire {
                out.push(Due {
                    key,
                    fire_at_ms,
                    occurrence_end_ms: ev.occurrence_end_ms,
                    title: ev.title.clone(),
                    location: ev.location.clone(),
                    conference_uri: ev.conference_uri.clone(),
                    is_all_day: ev.is_all_day,
                });
            }
        }
    }

    // Ties broken by the key so the order is total, not merely nondecreasing:
    // two reminders can share a fire-time (two calendars, one meeting), and an
    // unstable order there would make an assertion pass or fail on iteration
    // order alone.
    out.sort_by_key(|d| (d.fire_at_ms, d.key.event_id, d.key.occurrence_ms, d.key.minutes));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const MINUTE: i64 = 60_000;
    const HOUR: i64 = 60 * MINUTE;
    const HORIZON: i64 = 48 * HOUR;

    /// 2026-08-10T09:00:00Z.
    /// `python3 -c "import datetime as d;print(int(d.datetime(2026,8,10,9,tzinfo=d.timezone.utc).timestamp()*1000))"`
    const T0900Z: i64 = 1_786_352_400_000;

    const AUCKLAND: &str = "Pacific/Auckland";

    /// Midnight on 2026-08-10 in `Pacific/Auckland` — 2026-08-09T12:00Z. The
    /// answer the all-day test expects.
    ///
    /// Auckland is UTC+12 all August (New Zealand's DST runs late September to
    /// early April), so this instant's date read in **UTC** is 2026-08-09 — a
    /// different day. A fixture in `America/New_York` cannot tell those two
    /// readings apart: midnight there is 04:00Z on the *same* date.
    const AUCKLAND_MIDNIGHT_AUG10: i64 = 1_786_276_800_000;

    /// Midnight on 2026-08-10 in `Europe/Sofia` — 2026-08-09T21:00Z. The
    /// all-day test's *input*, and it is deliberately not the constant above.
    ///
    /// This is the divergence the store already models: an all-day event's
    /// `events.start_tz` is the zone Google sent (or the calendar's zone as it
    /// stood when sync ran), while `calendars.timezone` is the zone the
    /// calendar has *now*. `commands::occurrences` anchors an expanded all-day
    /// occurrence in `start_tz` — so a calendar whose zone has since changed,
    /// or an event Google stamped with a zone of its own, arrives here as
    /// midnight in a zone that is **not** `calendar_tz`. `omacal-store` has its
    /// own test for exactly that divergence
    /// (`events_in_window_returns_the_calendars_own_timezone_not_the_events`).
    ///
    /// Nine hours apart, and on the *same civil date* in both zones, so the
    /// all-day test below is purely a question of which midnight — not of which
    /// day.
    const SOFIA_MIDNIGHT_AUG10: i64 = 1_786_309_200_000;

    fn popup(minutes: i64) -> Reminder {
        Reminder { method: "popup".into(), minutes }
    }
    fn email(minutes: i64) -> Reminder {
        Reminder { method: "email".into(), minutes }
    }

    /// A timed, selected, single-occurrence event carrying its own overrides.
    /// Every test below starts here and changes only the field it is about.
    fn timed(event_id: i64, reminders: Vec<Reminder>) -> ScheduledEvent {
        ScheduledEvent {
            event_id,
            calendar_selected: true,
            calendar_tz: "UTC".into(),
            occurrence_start_ms: T0900Z,
            occurrence_end_ms: T0900Z + HOUR,
            is_all_day: false,
            use_default_reminders: false,
            overrides: reminders,
            calendar_defaults: Vec::new(),
            title: Some("Standup".into()),
            location: None,
            conference_uri: None,
        }
    }

    fn due(events: &[ScheduledEvent], now_ms: i64) -> Vec<Due> {
        due_reminders(events, &HashSet::new(), now_ms, HORIZON, &[])
    }

    /// `(event_id, minutes, fire_at_ms)` per result — the whole of what a
    /// caller acts on, in one comparable shape.
    fn triples(dues: &[Due]) -> Vec<(i64, i64, i64)> {
        dues.iter().map(|d| (d.key.event_id, d.key.minutes, d.fire_at_ms)).collect()
    }

    /// The arithmetic everything else rests on, and the positive control for
    /// every negative test below: without this, "produces nothing" would be
    /// satisfiable by a function that always produces nothing.
    #[test]
    fn a_reminder_is_due_its_minutes_before_the_occurrence_starts() {
        let out = due(&[timed(1, vec![popup(10)])], T0900Z - 2 * HOUR);
        assert_eq!(triples(&out), vec![(1, 10, T0900Z - 10 * MINUTE)]);
    }

    /// The fallback, in the direction Google uses most: `useDefault` means the
    /// calendar's list applies. The event also carries an override here, with
    /// different minutes, so an implementation that always reads `overrides`
    /// answers 10 and fails rather than coinciding.
    #[test]
    fn an_event_deferring_to_the_calendar_fires_the_calendars_defaults() {
        let mut ev = timed(1, vec![popup(10)]);
        ev.use_default_reminders = true;
        ev.calendar_defaults = vec![popup(30)];

        let out = due(&[ev], T0900Z - 2 * HOUR);
        assert_eq!(triples(&out), vec![(1, 30, T0900Z - 30 * MINUTE)]);
    }

    /// The other direction, and it needs its own fixture: an event with its own
    /// overrides must ignore the calendar's list *entirely* rather than merging
    /// the two. Google sends one or the other, never both.
    #[test]
    fn an_event_with_its_own_overrides_ignores_the_calendars_defaults() {
        let mut ev = timed(1, vec![popup(10)]);
        ev.use_default_reminders = false;
        ev.calendar_defaults = vec![popup(30)];

        let out = due(&[ev], T0900Z - 2 * HOUR);
        assert_eq!(triples(&out), vec![(1, 10, T0900Z - 10 * MINUTE)]);
    }

    /// Google sends the `email` ones itself. Firing them here would double
    /// every one.
    ///
    /// The fixture is deliberately mixed rather than email-only: an
    /// email-only event is also satisfied by a function that returns nothing
    /// at all, so on its own it is the weaker claim. Here the popup must
    /// survive and the email must not.
    #[test]
    fn only_popup_reminders_fire() {
        let out = due(&[timed(1, vec![popup(10), email(1440)])], T0900Z - 2 * HOUR);
        assert_eq!(triples(&out), vec![(1, 10, T0900Z - 10 * MINUTE)]);
    }

    /// The same rule stated on its own, because "Google already sent this one"
    /// is a different claim from "this event asked for nothing". An event whose
    /// only reminder is an email is one Google *will* deliver — it must simply
    /// not be delivered twice.
    #[test]
    fn an_event_whose_only_reminder_is_email_produces_nothing() {
        assert!(due(&[timed(1, vec![email(30)])], T0900Z - 2 * HOUR).is_empty());
    }

    /// Nothing is invented for an event that asked for nothing.
    #[test]
    fn an_event_with_no_reminders_produces_nothing() {
        assert!(due(&[timed(1, Vec::new())], T0900Z - 2 * HOUR).is_empty());
    }

    /// An all-day occurrence starts at midnight **in the calendar's own zone**,
    /// so "30 minutes before" is 23:30 the evening before, in that zone.
    ///
    /// The input is midnight in `Europe/Sofia` while the calendar is in
    /// `Pacific/Auckland`, and that choice is the whole test. An earlier
    /// version of this fixture fed an instant that was *already* midnight in
    /// the calendar's zone, which made the derivation the identity function:
    /// the rule could be deleted outright and every test still passed. A
    /// fixture that cannot distinguish "the rule ran" from "the rule was
    /// skipped" witnesses nothing, however carefully its other premises are
    /// asserted.
    ///
    /// The three premises below are what keep that from happening again, and
    /// the first is the one that was missing.
    #[test]
    fn an_all_day_occurrence_is_anchored_to_midnight_in_the_calendars_zone() {
        assert_ne!(
            SOFIA_MIDNIGHT_AUG10, AUCKLAND_MIDNIGHT_AUG10,
            "fixture check: the input must NOT already be the answer, or deleting \
             the derivation changes nothing and this test proves nothing"
        );
        assert_eq!(
            crate::zone::date_in_zone(SOFIA_MIDNIGHT_AUG10, AUCKLAND),
            "2026-08-10",
            "fixture check: input and answer share a civil date in the calendar's \
             zone, so this is a question of which midnight, not which day"
        );
        assert_eq!(
            crate::zone::date_in_zone(SOFIA_MIDNIGHT_AUG10, "UTC"),
            "2026-08-09",
            "fixture check: the same instant must fall on a different UTC date, \
             or this test cannot tell the calendar's zone from UTC"
        );

        let mut ev = timed(1, vec![popup(30)]);
        ev.is_all_day = true;
        ev.calendar_tz = AUCKLAND.into();
        ev.occurrence_start_ms = SOFIA_MIDNIGHT_AUG10;
        ev.occurrence_end_ms = SOFIA_MIDNIGHT_AUG10 + 24 * HOUR;

        let out = due(&[ev], AUCKLAND_MIDNIGHT_AUG10 - 2 * HOUR);
        assert_eq!(
            triples(&out),
            vec![(1, 30, AUCKLAND_MIDNIGHT_AUG10 - 30 * MINUTE)],
            "an all-day reminder must count back from midnight in the calendar's \
             own zone, not from the instant the expansion happened to produce"
        );
    }

    /// The occurrence key is the **anchor**, not the raw start, which is what
    /// makes a recorded reminder still match itself after a restart.
    ///
    /// Same divergent fixture as above: recording the anchor must suppress the
    /// reminder, and recording the raw instant must not, because the raw
    /// instant is not what this function computes.
    #[test]
    fn an_all_day_occurrence_is_keyed_by_its_anchor_not_its_raw_start() {
        let all_day = || {
            let mut ev = timed(1, vec![popup(30)]);
            ev.is_all_day = true;
            ev.calendar_tz = AUCKLAND.into();
            ev.occurrence_start_ms = SOFIA_MIDNIGHT_AUG10;
            ev.occurrence_end_ms = SOFIA_MIDNIGHT_AUG10 + 24 * HOUR;
            ev
        };
        let now = AUCKLAND_MIDNIGHT_AUG10 - 2 * HOUR;

        let by_anchor: HashSet<FiredKey> = [FiredKey {
            event_id: 1,
            occurrence_ms: AUCKLAND_MIDNIGHT_AUG10,
            minutes: 30,
        }]
        .into_iter()
        .collect();
        assert!(
            due_reminders(&[all_day()], &by_anchor, now, HORIZON, &[]).is_empty(),
            "a reminder recorded under the anchor must not be posted twice"
        );

        let by_raw_start: HashSet<FiredKey> = [FiredKey {
            event_id: 1,
            occurrence_ms: SOFIA_MIDNIGHT_AUG10,
            minutes: 30,
        }]
        .into_iter()
        .collect();
        assert_eq!(
            triples(&due_reminders(&[all_day()], &by_raw_start, now, HORIZON, &[])),
            vec![(1, 30, AUCKLAND_MIDNIGHT_AUG10 - 30 * MINUTE)],
            "the raw expansion instant is not the key this function computes"
        );
    }

    /// The other side of that branch: a timed occurrence keeps its own time of
    /// day and is never re-anchored to midnight. Without this, applying the
    /// all-day derivation unconditionally would fire every 09:00 meeting's
    /// reminder at half past midnight.
    #[test]
    fn a_timed_occurrence_is_not_re_anchored_to_midnight() {
        let mut ev = timed(1, vec![popup(30)]);
        ev.calendar_tz = AUCKLAND.into();

        let out = due(&[ev], T0900Z - 2 * HOUR);
        assert_eq!(triples(&out), vec![(1, 30, T0900Z - 30 * MINUTE)]);
    }

    /// §2.2: a calendar synced for completeness but hidden from the grid does
    /// not get to interrupt you. Deselecting cancels its pending reminders, and
    /// that is the intended reading of the switch.
    #[test]
    fn only_selected_calendars_fire() {
        let shown = timed(1, vec![popup(10)]);
        let mut hidden = timed(2, vec![popup(10)]);
        hidden.calendar_selected = false;

        let out = due(&[shown, hidden], T0900Z - 2 * HOUR);
        assert_eq!(triples(&out), vec![(1, 10, T0900Z - 10 * MINUTE)]);
    }

    /// The restart case. Without this every launch re-posts every reminder for
    /// every event still in the window.
    ///
    /// The second event is what stops this passing against a function that
    /// returns nothing.
    #[test]
    fn an_already_fired_reminder_is_not_returned_again() {
        let events = [timed(1, vec![popup(10)]), timed(2, vec![popup(10)])];
        let fired: HashSet<FiredKey> =
            [FiredKey { event_id: 1, occurrence_ms: T0900Z, minutes: 10 }].into_iter().collect();

        let out = due_reminders(&events, &fired, T0900Z - 2 * HOUR, HORIZON, &[]);
        assert_eq!(triples(&out), vec![(2, 10, T0900Z - 10 * MINUTE)]);
    }

    /// Keyed by occurrence, so a weekly standup fires every week. Both
    /// occurrences here are the same `event_id`; recording one must not silence
    /// the next.
    #[test]
    fn recording_one_occurrence_does_not_silence_the_next() {
        let first = timed(1, vec![popup(10)]);
        let mut second = timed(1, vec![popup(10)]);
        second.occurrence_start_ms = T0900Z + 7 * 24 * HOUR;
        second.occurrence_end_ms = T0900Z + 7 * 24 * HOUR + HOUR;

        let fired: HashSet<FiredKey> =
            [FiredKey { event_id: 1, occurrence_ms: T0900Z, minutes: 10 }].into_iter().collect();

        let out = due_reminders(
            &[first, second],
            &fired,
            T0900Z - 2 * HOUR,
            8 * 24 * HOUR,
            &[],
        );
        assert_eq!(triples(&out), vec![(1, 10, T0900Z + 7 * 24 * HOUR - 10 * MINUTE)]);
    }

    /// And keyed by minutes, so an event's 10-minute and 1-minute reminders are
    /// separate: posting the first must not swallow the second.
    #[test]
    fn recording_one_reminder_does_not_silence_the_events_other_one() {
        let fired: HashSet<FiredKey> =
            [FiredKey { event_id: 1, occurrence_ms: T0900Z, minutes: 10 }].into_iter().collect();

        let out = due_reminders(
            &[timed(1, vec![popup(10), popup(1)])],
            &fired,
            T0900Z - 2 * HOUR,
            HORIZON,
            &[],
        );
        assert_eq!(triples(&out), vec![(1, 1, T0900Z - MINUTE)]);
    }

    /// The horizon window: a fire-time further out than `horizon_ms` is not
    /// returned *yet*. It will be, by the recomputation that lands inside it.
    ///
    /// The horizon is measured against the **fire time**, not the occurrence
    /// start — an event beginning just past the horizon with a long lead time
    /// has a reminder inside it, and that reminder is due.
    ///
    /// Three events pin the whole edge: inside, exactly on it, and beyond.
    /// Exactly on the horizon is excluded, so the window is half-open —
    /// `[now, now + horizon)`, the same convention as `events_in_window`.
    #[test]
    fn a_fire_time_beyond_the_horizon_is_not_returned_yet() {
        let now = T0900Z;

        let mut inside = timed(1, vec![popup(10)]);
        inside.occurrence_start_ms = now + HOUR;
        inside.occurrence_end_ms = now + 2 * HOUR;

        // Fire time lands exactly on `now + HORIZON`.
        let mut on_the_edge = timed(2, vec![popup(10)]);
        on_the_edge.occurrence_start_ms = now + HORIZON + 10 * MINUTE;
        on_the_edge.occurrence_end_ms = on_the_edge.occurrence_start_ms + HOUR;

        let mut beyond = timed(3, vec![popup(10)]);
        beyond.occurrence_start_ms = now + 3 * 24 * HOUR;
        beyond.occurrence_end_ms = beyond.occurrence_start_ms + HOUR;

        let out = due(&[inside, on_the_edge, beyond], now);
        assert_eq!(triples(&out), vec![(1, 10, now + HOUR - 10 * MINUTE)]);
    }

    /// §2.3, the missed-reminder rule: a fire-time that passed while omacal was
    /// not running still fires, so long as the event has not ended. You hear
    /// about a meeting that is starting or already in progress.
    ///
    /// A separate condition from the horizon above, and separately tested: this
    /// one's fire-time is in the *past*, which the horizon check never sees.
    #[test]
    fn a_reminder_whose_time_has_passed_fires_while_the_event_is_still_running() {
        let ev = timed(1, vec![popup(10)]);
        // Five minutes in: the reminder came due 15 minutes ago and the
        // meeting is on right now.
        let now = T0900Z + 5 * MINUTE;

        let out = due(&[ev], now);
        assert_eq!(triples(&out), vec![(1, 10, T0900Z - 10 * MINUTE)]);
    }

    /// The cutoff on that same rule. Opening the laptop after a weekend must
    /// not bury you in alerts for meetings that are over.
    #[test]
    fn a_reminder_for_an_event_that_has_already_ended_is_dropped() {
        let ev = timed(1, vec![popup(10)]);
        // One millisecond past the end, which is the tightest statement of
        // "over": at exactly `end` the event is no longer running either.
        let out = due(std::slice::from_ref(&ev), ev.occurrence_end_ms);
        assert!(out.is_empty(), "a finished meeting must not still notify");
    }

    /// Each result carries the end of the occurrence it belongs to — the
    /// instant past which it can never be produced again, and so the only
    /// correct thing for a driver to prune its record on. Taken from the
    /// occurrence rather than derived from the anchor: an all-day occurrence
    /// runs a day from its raw start, not a day from the midnight the anchor
    /// normalised it to, and the two differ whenever the calendar's zone and
    /// the event's stored zone do.
    #[test]
    fn each_result_carries_the_end_of_the_occurrence_it_belongs_to() {
        let mut ev = timed(1, vec![popup(30)]);
        ev.is_all_day = true;
        ev.calendar_tz = AUCKLAND.into();
        ev.occurrence_start_ms = SOFIA_MIDNIGHT_AUG10;
        ev.occurrence_end_ms = SOFIA_MIDNIGHT_AUG10 + 24 * HOUR;

        let out = due(&[ev], AUCKLAND_MIDNIGHT_AUG10 - 2 * HOUR);
        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0].occurrence_end_ms,
            SOFIA_MIDNIGHT_AUG10 + 24 * HOUR,
            "the occurrence's own end, not one derived from the anchor"
        );
        assert_ne!(
            out[0].occurrence_end_ms,
            AUCKLAND_MIDNIGHT_AUG10 + 24 * HOUR,
            "fixture check: an anchor-derived end must be a different instant, \
             or this test cannot tell the two apart"
        );
    }

    /// Ordered by fire time, earliest first, so the driver can take the head
    /// rather than scanning — and so every assertion above compares against a
    /// stable order rather than input order. The fixture is built backwards on
    /// purpose: input order and fire order disagree.
    #[test]
    fn results_are_ordered_by_fire_time() {
        let late = timed(1, vec![popup(1)]);
        let early = timed(2, vec![popup(45)]);
        let middle = timed(3, vec![popup(10)]);

        let out = due(&[late, early, middle], T0900Z - 2 * HOUR);
        assert_eq!(
            triples(&out),
            vec![
                (2, 45, T0900Z - 45 * MINUTE),
                (3, 10, T0900Z - 10 * MINUTE),
                (1, 1, T0900Z - MINUTE),
            ]
        );
    }

    // --- The fallback (fallback spec §1) ---------------------------------

    /// The one positive case, leading as the control: without it, the three
    /// negatives below are satisfied by a fallback that never applies at all.
    #[test]
    fn the_fallback_fills_empty_defaults_on_a_timed_occurrence() {
        let mut ev = timed(1, vec![]);
        ev.use_default_reminders = true;
        let fb = vec![popup(60), popup(10)];

        let out = due_reminders(&[ev], &HashSet::new(), T0900Z - 2 * HOUR, HORIZON, &fb);
        assert_eq!(
            triples(&out),
            vec![(1, 60, T0900Z - HOUR), (1, 10, T0900Z - 10 * MINUTE)]
        );
    }

    #[test]
    fn real_calendar_defaults_keep_the_fallback_out() {
        let mut ev = timed(1, vec![]);
        ev.use_default_reminders = true;
        ev.calendar_defaults = vec![popup(30)];

        let out = due_reminders(
            &[ev],
            &HashSet::new(),
            T0900Z - 2 * HOUR,
            HORIZON,
            &[popup(60), popup(10)],
        );
        assert_eq!(triples(&out), vec![(1, 30, T0900Z - 30 * MINUTE)]);
    }

    /// `use_default: false, overrides: []` is "no reminders", chosen on
    /// purpose — the form writes exactly that when every row is removed — and
    /// the fallback must not overrule a person who said none.
    #[test]
    fn empty_overrides_mean_none_and_the_fallback_respects_that() {
        let ev = timed(1, vec![]);
        let out =
            due_reminders(&[ev], &HashSet::new(), T0900Z - 2 * HOUR, HORIZON, &[popup(10)]);
        assert!(out.is_empty());
    }

    /// All-day occurrences are excluded (spec §1): their anchor is midnight,
    /// and "60 minutes before" would knock at 23:00 the night before every
    /// trip and holiday on the calendar.
    #[test]
    fn the_fallback_never_touches_an_all_day_occurrence() {
        let mut ev = timed(1, vec![]);
        ev.use_default_reminders = true;
        ev.is_all_day = true;
        ev.occurrence_end_ms = T0900Z + 24 * HOUR;

        let out =
            due_reminders(&[ev], &HashSet::new(), T0900Z - 2 * HOUR, HORIZON, &[popup(60)]);
        assert!(out.is_empty());
    }
}
