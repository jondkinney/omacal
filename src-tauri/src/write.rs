//! Pure builders for event write bodies.
//!
//! Everything here is a function of its arguments: no pool, no client, no
//! clock. The write commands stay thin wrappers around these so the rules
//! that matter — "never send a field the user did not touch", "all-day means
//! `date` not `dateTime`" — are testable without a server.

use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EventFields {
    pub summary: Option<String>,
    pub location: Option<String>,
    pub description: Option<String>,
    pub start_ms: i64,
    pub end_ms: i64,
    pub is_all_day: bool,
    /// IANA zone the times are authored in.
    pub tz: String,
    /// Three-state, and the distinction is the point:
    /// `None` — the user did not touch Repeat; omit `recurrence` entirely.
    /// `Some(None)` — the user chose Never; send `null`.
    /// `Some(Some(rule))` — send `[rule]`.
    pub recurrence: Option<Option<String>>,
}

/// Google's `start`/`end` object. All-day events carry `date`; timed events
/// carry `dateTime` and `timeZone`. Sending both is rejected.
///
/// An unresolvable timestamp or zone must still produce a date rather than
/// panic — the all-day branch falls back to the Unix epoch and then to UTC,
/// same as the fallback philosophy used elsewhere for zone handling
/// (`n_day_boundaries`, `local_midnight_ms` in `commands.rs`).
pub(crate) fn event_time_json(ms: i64, is_all_day: bool, tz: &str) -> Value {
    if is_all_day {
        let ts = jiff::Timestamp::from_millisecond(ms).unwrap_or(jiff::Timestamp::UNIX_EPOCH);
        let zoned = ts
            .in_tz(tz)
            .unwrap_or_else(|_| ts.in_tz("UTC").expect("UTC always resolves"));
        json!({ "date": zoned.date().to_string() })
    } else {
        json!({ "dateTime": omacal_sync::to_rfc3339(ms), "timeZone": tz })
    }
}

/// Where `target_ms` lands after the same *calendar* movement that takes
/// `from_ms` to `to_ms`, read in `tz`.
///
/// Deliberately not `target_ms + (to_ms - from_ms)`. An edit is applied to a
/// resource that may be anchored a long way from the occurrence the user was
/// looking at — a series master months earlier — and a daylight-saving
/// transition can sit between the two. A millisecond delta then carries the
/// transition across with it: moving an occurrence from Saturday to Sunday
/// over a spring-forward is 23 hours, and 23 hours added to a master on the
/// other side of it arrives an hour early. For an all-day event that is worse
/// than untidy — 23 hours from midnight is 23:00 the same day, so the whole
/// move is silently discarded, while a PATCH still goes out with
/// `sendUpdates=all` and mails every guest about a change that did not
/// happen.
///
/// So the movement is measured *civilly*: the span between two wall clocks,
/// which `jiff` balances into days plus a time of day, added to the target's
/// own wall clock and only then resolved back to an instant. A day stays a
/// day across a transition; an hour stays an hour.
///
/// The two short circuits are not optimisations, they are guarantees. Nothing
/// moved means the target does not move, whatever `tz` says — that is what
/// makes "an untouched time sends no `start`/`end`" exact rather than
/// approximate. And when the target *is* the thing that moved (every one-off,
/// and every resolved occurrence) the answer is the new instant itself, with
/// no civil round trip that a repeated hour could shift.
///
/// An unresolvable zone falls back to the plain delta rather than failing, the
/// same fallback philosophy as [`event_time_json`].
pub(crate) fn shifted_like(target_ms: i64, from_ms: i64, to_ms: i64, tz: &str) -> i64 {
    if to_ms == from_ms {
        return target_ms;
    }
    if target_ms == from_ms {
        return to_ms;
    }

    let civil = |ms: i64| -> Option<jiff::civil::DateTime> {
        jiff::Timestamp::from_millisecond(ms).ok()?.in_tz(tz).ok().map(|z| z.datetime())
    };
    let moved = (|| -> Option<i64> {
        let movement = civil(to_ms)? - civil(from_ms)?;
        Some(
            civil(target_ms)?
                .checked_add(movement)
                .ok()?
                .in_tz(tz)
                .ok()?
                .timestamp()
                .as_millisecond(),
        )
    })();
    moved.unwrap_or_else(|| target_ms.saturating_add(to_ms.saturating_sub(from_ms)))
}

/// A PATCH body carrying only what actually changed.
///
/// A field absent from a PATCH body means "leave it alone"; a field present
/// and null means "clear it". Both are needed, and conflating them makes
/// clearing a location impossible.
///
/// `create_event` builds its insert body from `EventFields` directly instead —
/// a create has no "before" to diff against. The edit command is this
/// function's consumer: `events::edit_patch_body` builds both sides and calls
/// it, and its doc comment is where the rules for *how* each side is built
/// live.
pub(crate) fn changed_fields(before: &EventFields, after: &EventFields) -> Value {
    let mut body = serde_json::Map::new();

    let mut text = |key: &str, b: &Option<String>, a: &Option<String>| {
        if b != a {
            body.insert(
                key.to_string(),
                match a {
                    Some(s) => Value::String(s.clone()),
                    None => Value::Null,
                },
            );
        }
    };
    text("summary", &before.summary, &after.summary);
    text("location", &before.location, &after.location);
    text("description", &before.description, &after.description);

    // Times move as a pair. Google rejects a body that redefines one end of
    // an event without the other when the all-day flag is in play, and half a
    // move is not a thing a user can mean. `tz` is in this trigger too: it
    // never appears in the body by itself, but it changes what `dateTime`/
    // `date` serialize to, so a zone-only edit must still resend both ends.
    if before.start_ms != after.start_ms
        || before.end_ms != after.end_ms
        || before.is_all_day != after.is_all_day
        || before.tz != after.tz
    {
        body.insert(
            "start".into(),
            event_time_json(after.start_ms, after.is_all_day, &after.tz),
        );
        body.insert(
            "end".into(),
            event_time_json(after.end_ms, after.is_all_day, &after.tz),
        );
    }

    match &after.recurrence {
        None => {}
        Some(None) => {
            body.insert("recurrence".into(), Value::Null);
        }
        Some(Some(rule)) => {
            body.insert("recurrence".into(), json!([rule]));
        }
    }

    Value::Object(body)
}

/// What the UI actually sends. Distinct from [`EventFields`] because the
/// three-state above needs two levels of `Option` and JSON has one `null`.
///
/// `repeat` carries the dropdown's own vocabulary rather than an RRULE: the UI
/// has no business authoring iCalendar, and keeping the mapping in one place
/// ([`rrule_for`]) is what makes "a rule we cannot express is never
/// overwritten" checkable.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EventInput {
    pub summary: Option<String>,
    pub location: Option<String>,
    pub description: Option<String>,
    pub start_ms: i64,
    pub end_ms: i64,
    pub is_all_day: bool,
    pub tz: String,
    /// Absent when the user did not touch Repeat.
    #[serde(default)]
    pub repeat: Option<String>,
}

/// `"never"` maps to `Some(None)` — clear the rule — because [`rrule_for`]
/// returns `None` for it. That is the one case worth staring at: an absent
/// `repeat` and a `repeat` of `"never"` must not collapse together.
pub(crate) fn fields_from_input(input: EventInput) -> EventFields {
    EventFields {
        summary: input.summary,
        location: input.location,
        description: input.description,
        start_ms: input.start_ms,
        end_ms: input.end_ms,
        is_all_day: input.is_all_day,
        tz: input.tz,
        recurrence: input.repeat.map(|r| rrule_for(&r)),
    }
}

/// The rule omacal writes for each Repeat option. `never` is `None`.
pub(crate) fn rrule_for(repeat: &str) -> Option<String> {
    Some(
        match repeat {
            "daily" => "RRULE:FREQ=DAILY",
            "weekdays" => "RRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR",
            "weekly" => "RRULE:FREQ=WEEKLY",
            "monthly" => "RRULE:FREQ=MONTHLY",
            "yearly" => "RRULE:FREQ=YEARLY",
            _ => return None,
        }
        .to_string(),
    )
}

/// Which Repeat option, if any, represents `rule` exactly.
///
/// Exact string equality against what [`rrule_for`] authors, deliberately —
/// not a parse. A rule carrying `INTERVAL`, `COUNT`, `UNTIL`, `EXDATE` or a
/// `BYDAY` we did not write is `custom`, and the UI must then refuse to
/// overwrite it. Being generous here (parsing `FREQ` and ignoring the rest)
/// is exactly how "every 2nd Tuesday" becomes "weekly" behind the user's back.
///
/// Only the Repeat control's read side (Task 9) will call this, to decide
/// whether the rule on an existing event can be represented at all — no
/// write command needs it. Unused outside its own tests until then.
#[allow(dead_code)]
pub(crate) fn repeat_from_rrule(rule: Option<&str>) -> String {
    let Some(rule) = rule else {
        return "never".into();
    };
    for candidate in ["daily", "weekdays", "weekly", "monthly", "yearly"] {
        if rrule_for(candidate).as_deref() == Some(rule) {
            return candidate.into();
        }
    }
    "custom".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> EventFields {
        EventFields {
            summary: Some("Standup".into()),
            location: None,
            description: None,
            start_ms: 1_785_398_400_000,
            end_ms: 1_785_400_200_000,
            is_all_day: false,
            tz: "Europe/Sofia".into(),
            recurrence: None,
        }
    }

    /// The property the whole module exists for. A fortnightly meeting whose
    /// title changes must not carry `recurrence` in its body — the Repeat
    /// dropdown cannot express "every 2nd Tuesday", so sending it would
    /// silently rewrite the real rule to something simpler.
    #[test]
    fn an_untouched_recurrence_is_never_sent() {
        let mut after = base();
        after.summary = Some("Standup (moved)".into());
        let body = changed_fields(&base(), &after);
        assert_eq!(body["summary"], "Standup (moved)");
        assert!(body.get("recurrence").is_none(), "body was {body}");
    }

    #[test]
    fn nothing_changed_produces_an_empty_body() {
        assert_eq!(changed_fields(&base(), &base()), serde_json::json!({}));
    }

    /// Clearing a field must send explicit null, not omit it — omitting means
    /// "leave alone" to a PATCH, so a cleared location would silently persist.
    #[test]
    fn clearing_a_field_sends_null_rather_than_omitting_it() {
        let mut before = base();
        before.location = Some("Room 4A".into());
        let body = changed_fields(&before, &base());
        assert!(body.get("location").is_some(), "body was {body}");
        assert!(body["location"].is_null());
    }

    /// Google rejects a body with start but not end when only one moved, and
    /// a half-moved event is meaningless anyway.
    #[test]
    fn moving_either_end_sends_both_times() {
        let mut after = base();
        after.end_ms += 900_000;
        let body = changed_fields(&base(), &after);
        assert!(body.get("start").is_some(), "body was {body}");
        assert!(body.get("end").is_some(), "body was {body}");
    }

    #[test]
    fn a_touched_repeat_is_sent_as_an_array() {
        let mut after = base();
        after.recurrence = Some(Some("RRULE:FREQ=WEEKLY".into()));
        let body = changed_fields(&base(), &after);
        assert_eq!(body["recurrence"], serde_json::json!(["RRULE:FREQ=WEEKLY"]));
    }

    /// Turning repetition off is `recurrence: null`, which Google reads as
    /// "make this a single event". `Value`'s `Index` returns `Null` for a
    /// *missing* key too, so the presence check is load-bearing: without it
    /// this test cannot tell "sent null" from "never mentioned recurrence".
    #[test]
    fn repeat_set_to_never_sends_null() {
        let mut after = base();
        after.recurrence = Some(None);
        let body = changed_fields(&base(), &after);
        assert!(body.get("recurrence").is_some(), "body was {body}");
        assert!(body["recurrence"].is_null());
    }

    /// A zone-only edit (same wall-clock times, different tz) still changes
    /// what `dateTime`/`date` serialize to, so it must not be dropped just
    /// because `start_ms`/`end_ms`/`is_all_day` are unchanged.
    #[test]
    fn a_timezone_only_change_still_sends_both_times() {
        let mut after = base();
        after.tz = "America/New_York".into();
        let body = changed_fields(&base(), &after);
        assert!(body.get("start").is_some(), "body was {body}");
        assert!(body.get("end").is_some(), "body was {body}");
        assert_eq!(body["start"]["timeZone"], "America/New_York");
    }

    #[test]
    fn a_timed_event_sends_datetime_and_zone() {
        let v = event_time_json(1_785_398_400_000, false, "Europe/Sofia");
        assert!(v["dateTime"].is_string());
        assert_eq!(v["timeZone"], "Europe/Sofia");
        assert!(v.get("date").is_none());
    }

    /// All-day events use `date`, never `dateTime` — Google rejects the mix.
    #[test]
    fn an_all_day_event_sends_a_bare_date() {
        let v = event_time_json(1_785_398_400_000, true, "Europe/Sofia");
        assert!(v["date"].is_string());
        assert_eq!(v["date"].as_str().unwrap().len(), 10);
        assert!(v.get("dateTime").is_none());
    }

    /// An unresolvable zone must fall back to UTC rather than panic — the
    /// simplified fallback chain in `event_time_json` still has to hold this.
    #[test]
    fn an_unknown_timezone_falls_back_to_a_date_instead_of_panicking() {
        let v = event_time_json(1_785_398_400_000, true, "Not/AZone");
        assert!(v["date"].is_string());
        assert_eq!(v["date"].as_str().unwrap().len(), 10);
    }

    /// A wall clock in New York as an instant. `2026-03-08T02:00` is that
    /// zone's spring-forward for 2026, which is what the tests below sit
    /// either side of.
    fn ny(wall: &str) -> i64 {
        wall.parse::<jiff::civil::DateTime>()
            .unwrap()
            .in_tz("America/New_York")
            .unwrap()
            .timestamp()
            .as_millisecond()
    }

    /// The property `shifted_like` exists for. Moving an occurrence from the
    /// Saturday before a spring-forward to the Sunday after it is 23 hours of
    /// elapsed time but one day of calendar time. Applied to a master a month
    /// earlier — on the winter side of the transition — the plain delta
    /// arrives an hour early and the meeting silently moves to 08:00.
    #[test]
    fn a_day_stays_a_day_when_the_shift_crosses_a_daylight_saving_transition() {
        let master = ny("2026-02-07T09:00:00");
        let occurrence = ny("2026-03-07T09:00:00");
        let moved = ny("2026-03-08T09:00:00");

        assert_eq!(
            moved - occurrence,
            23 * 3_600_000,
            "fixture check: this move must actually cross the transition, or the \
             assertion below proves nothing"
        );
        assert_eq!(
            shifted_like(master, occurrence, moved, "America/New_York"),
            ny("2026-02-08T09:00:00"),
            "a one-day move became 23 hours: the series would keep its time of day \
             only on the side of the transition it was edited from"
        );
    }

    /// The same shift, on an all-day event, where the damage is worse: 23
    /// hours from midnight is 23:00 the same day, so `event_time_json` renders
    /// the *original* date and the user's move vanishes — while the body still
    /// differs in milliseconds, so a PATCH goes out and every guest is told
    /// about a change that did not happen.
    #[test]
    fn an_all_day_shift_across_a_transition_lands_on_the_next_date_not_the_same_one() {
        let master = ny("2026-02-07T00:00:00");
        let occurrence = ny("2026-03-08T00:00:00");
        let moved = ny("2026-03-09T00:00:00");

        let shifted = shifted_like(master, occurrence, moved, "America/New_York");
        assert_eq!(shifted, ny("2026-02-08T00:00:00"));
        assert_eq!(
            event_time_json(shifted, true, "America/New_York")["date"],
            "2026-02-08",
            "the move was dropped: the body would re-send the date the event already has"
        );
    }

    /// The control. Without it the two tests above could pass for a reason
    /// that has nothing to do with transitions — a function that always
    /// returned "the same wall clock, one day on" would satisfy them both.
    #[test]
    fn an_ordinary_shift_with_no_transition_in_it_still_moves_by_what_the_user_did() {
        let master = ny("2026-06-06T09:00:00");
        let occurrence = ny("2026-07-04T09:00:00");

        // A pure time-of-day change.
        assert_eq!(
            shifted_like(master, occurrence, occurrence + 90 * 60_000, "America/New_York"),
            ny("2026-06-06T10:30:00")
        );
        // A day *and* a time-of-day change together.
        assert_eq!(
            shifted_like(master, occurrence, ny("2026-07-06T08:00:00"), "America/New_York"),
            ny("2026-06-08T08:00:00")
        );
    }

    /// Both short circuits, which are guarantees rather than optimisations —
    /// see the doc comment. The first is what makes "an untouched time sends
    /// nothing" exact; the second keeps every one-off and every resolved
    /// occurrence away from a civil round trip that a repeated hour could
    /// shift.
    #[test]
    fn nothing_moved_moves_nothing_and_a_target_that_is_itself_the_move_takes_the_new_instant() {
        let target = ny("2026-02-07T09:00:00");
        let from = ny("2026-03-07T09:00:00");
        assert_eq!(shifted_like(target, from, from, "America/New_York"), target);
        let to = ny("2026-03-08T09:00:00");
        assert_eq!(shifted_like(from, from, to, "America/New_York"), to);
    }

    /// An unresolvable zone must not panic or swallow the movement; it falls
    /// back to the plain delta, exactly as `event_time_json` falls back to UTC.
    #[test]
    fn an_unknown_timezone_falls_back_to_the_plain_delta() {
        assert_eq!(shifted_like(1_000, 5_000, 8_000, "Not/AZone"), 4_000);
    }

    fn sample_input() -> EventInput {
        EventInput {
            summary: Some("Standup".into()),
            location: None,
            description: None,
            start_ms: 1_785_398_400_000,
            end_ms: 1_785_400_200_000,
            is_all_day: false,
            tz: "Europe/Sofia".into(),
            repeat: None,
        }
    }

    #[test]
    fn each_offered_repeat_maps_to_a_rule() {
        assert_eq!(rrule_for("never"), None);
        assert_eq!(rrule_for("daily").as_deref(), Some("RRULE:FREQ=DAILY"));
        assert_eq!(
            rrule_for("weekdays").as_deref(),
            Some("RRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR")
        );
        assert_eq!(rrule_for("weekly").as_deref(), Some("RRULE:FREQ=WEEKLY"));
        assert_eq!(rrule_for("monthly").as_deref(), Some("RRULE:FREQ=MONTHLY"));
        assert_eq!(rrule_for("yearly").as_deref(), Some("RRULE:FREQ=YEARLY"));
    }

    #[test]
    fn every_rule_we_author_reads_back_as_itself() {
        for r in ["daily", "weekdays", "weekly", "monthly", "yearly"] {
            let rule = rrule_for(r).unwrap();
            assert_eq!(repeat_from_rrule(Some(&rule)), r, "round trip failed for {r}");
        }
        assert_eq!(repeat_from_rrule(None), "never");
    }

    /// The property that stops a silent overwrite: a rule the dropdown cannot
    /// express must be reported as `custom`, so the UI can disable the control
    /// rather than offering to replace it with something simpler.
    #[test]
    fn a_rule_we_cannot_express_is_custom() {
        for exotic in [
            "RRULE:FREQ=MONTHLY;BYDAY=-1FR",
            "RRULE:FREQ=WEEKLY;INTERVAL=2",
            "RRULE:FREQ=DAILY;COUNT=5",
            "RRULE:FREQ=WEEKLY;BYDAY=MO,WE",
            "RRULE:FREQ=WEEKLY;UNTIL=20261231T000000Z",
        ] {
            assert_eq!(repeat_from_rrule(Some(exotic)), "custom", "{exotic}");
        }
    }

    /// The two states JSON cannot tell apart on its own, and the reason
    /// `EventInput` exists. An absent `repeat` must leave the rule alone; an
    /// explicit `"never"` must clear it. Collapsing them makes every title edit
    /// on a recurring event either impossible or destructive.
    #[test]
    fn an_absent_repeat_and_an_explicit_never_are_different_things() {
        let mut input = sample_input();
        input.repeat = None;
        assert_eq!(fields_from_input(input).recurrence, None);

        let mut input = sample_input();
        input.repeat = Some("never".into());
        assert_eq!(fields_from_input(input).recurrence, Some(None));

        let mut input = sample_input();
        input.repeat = Some("weekly".into());
        assert_eq!(
            fields_from_input(input).recurrence,
            Some(Some("RRULE:FREQ=WEEKLY".into()))
        );
    }
}
