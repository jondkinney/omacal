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

/// When an event happens.
///
/// An all-day event has **no instant and no zone** — Google models it as a bare
/// `date`, and so does the store once `omacal_sync::resolve` has read it. The
/// enum exists so nobody can supply a zone for one: the previous shape took
/// `(ms, is_all_day, tz)` and the two sides of the boundary converted that date
/// to an instant in *different* zones, which moved events nobody moved.
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)] // wired up in Task 2, which retires `event_time_json`.
pub(crate) enum When {
    Timed { start_ms: i64, end_ms: i64 },
    /// Both `yyyy-mm-dd`. `end_date` is **exclusive** — the day after the last
    /// one — matching Google's wire format and the store's `end_utc`.
    AllDay { start_date: String, end_date: String },
}

/// Google's `start` and `end` objects. `tz` is read only by the timed arm.
#[allow(dead_code)] // wired up in Task 2, which retires `event_time_json`.
pub(crate) fn when_json(when: &When, tz: &str) -> (Value, Value) {
    match when {
        When::AllDay { start_date, end_date } => (
            json!({ "date": start_date }),
            json!({ "date": end_date }),
        ),
        When::Timed { start_ms, end_ms } => (
            json!({ "dateTime": omacal_sync::to_rfc3339(*start_ms), "timeZone": tz }),
            json!({ "dateTime": omacal_sync::to_rfc3339(*end_ms),   "timeZone": tz }),
        ),
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

/// The `UNTIL` value that ends a series immediately before `before_ms`.
///
/// Two forms, because RFC 5545 §3.3.10 requires `UNTIL` to carry the same
/// *value type* as `DTSTART`, and getting that wrong produces a rule other
/// clients reject even where Google is lenient about it.
///
/// A timed series has a `DTSTART` with a zone, so `UNTIL` must be a UTC
/// date-time: `before_ms` less one second, rendered `%Y%m%dT%H%M%SZ`. One
/// second, because `UNTIL` is *inclusive* — landing it on the occurrence
/// itself keeps that occurrence in both series, and a whole minute earlier
/// would drop any occurrence in between.
///
/// An all-day series has a `DTSTART` that is a bare date, so `UNTIL` must be a
/// bare date too — and "the moment before" is then the *previous day*, not the
/// previous second, since a date-valued `UNTIL` is inclusive of the whole day
/// it names. The date is read in `tz`.
///
/// **`is_all_day` is the value type of the series being truncated, and of
/// nothing else.** Worth stating flatly, because the plausible-sounding wrong
/// answer is right next to it: the caller (`events::split_series`) creates a
/// *second* event in the same breath, and that one has an all-day flag of its
/// own — the form's. They are unrelated. A truncation patches `recurrence` and
/// nothing else, so the event it lands on keeps whatever `start` it already
/// had; the rule has to agree with *that*. A user splitting an all-day series
/// into a timed remainder is an ordinary thing to do, and taking the new
/// event's flag there writes a date-time `UNTIL` onto a series whose `DTSTART`
/// is still a bare date. An earlier version of this comment said the date was
/// read in the same zone the sibling `start` is rendered in — true of the
/// fixtures, false as a rule, and an instruction to introduce exactly that bug.
///
/// **No daylight-saving hazard, unlike [`shifted_like`].** The timed form is an
/// absolute UTC instant on both sides: one second is subtracted from epoch
/// milliseconds and rendered in UTC, with no wall clock and no zone in
/// between, so no transition can move it. The all-day form does read a wall
/// clock, but only to name a calendar date — and a date is what a transition
/// leaves alone. `shifted_like` needed civil arithmetic precisely because it
/// carries a *span* across a transition; nothing here carries a span.
fn until_value(before_ms: i64, is_all_day: bool, tz: &str) -> String {
    let at = |ms: i64| {
        let ts = jiff::Timestamp::from_millisecond(ms).unwrap_or(jiff::Timestamp::UNIX_EPOCH);
        ts.in_tz(tz).unwrap_or_else(|_| ts.in_tz("UTC").expect("UTC always resolves"))
    };
    if is_all_day {
        let date = at(before_ms).date();
        // `yesterday` fails only at the very start of the supported range.
        date.yesterday().unwrap_or(date).strftime("%Y%m%d").to_string()
    } else {
        jiff::Timestamp::from_millisecond(before_ms.saturating_sub(1000))
            .unwrap_or(jiff::Timestamp::UNIX_EPOCH)
            .in_tz("UTC")
            .expect("UTC always resolves")
            .strftime("%Y%m%dT%H%M%SZ")
            .to_string()
    }
}

/// One `RRULE` line, ending strictly before `before_ms`.
///
/// This is half of "this and following": the original series is shortened to
/// stop just before the occurrence the user split at, and a new series carries
/// the rest. `events::split_series` is the other half.
///
/// Three rewrites, each of which produces an invalid or wrong rule if skipped:
///
/// * **`UNTIL` is added** at [`until_value`]'s instant — see there for why it
///   is one second (or one day) before rather than on the occurrence.
/// * **An existing `UNTIL` is replaced, not appended.** Two `UNTIL` parts in
///   one rule is not valid iCalendar, and which one a parser honours is
///   anyone's guess.
/// * **`COUNT` is dropped.** RFC 5545 §3.3.10 makes `COUNT` and `UNTIL`
///   mutually exclusive: "the UNTIL and COUNT rule parts are OPTIONAL, but
///   they MUST NOT occur in the same 'recur'". A rule carrying both is
///   rejected outright by some parsers and silently resolved by others.
///
/// Everything else is carried through untouched and in its original order —
/// `INTERVAL`, `BYDAY`, `WKST`, and any part this app does not model. The
/// `RRULE:` prefix is split off first rather than matched as text, so a rule
/// whose first part happens to be `UNTIL` is still rewritten rather than
/// having it hide behind the prefix.
///
/// Parts are matched on their key, case-insensitively: RFC 5545's ABNF spells
/// them uppercase and Google always emits them that way, but a rule that
/// arrived from another client is not this function's to trust.
pub(crate) fn truncated_rule(rule: &str, before_ms: i64, is_all_day: bool, tz: &str) -> String {
    let (prefix, parts) = match rule.split_once(':') {
        Some((name, parts)) => (format!("{name}:"), parts),
        None => (String::new(), rule),
    };

    let mut kept: Vec<String> = parts
        .split(';')
        .filter(|part| {
            let key = part.split('=').next().unwrap_or_default();
            !key.eq_ignore_ascii_case("UNTIL") && !key.eq_ignore_ascii_case("COUNT")
        })
        .map(str::to_string)
        .collect();
    // Not `parts.split(';')` straight into `format!`: a rule that was nothing
    // but a `COUNT` would leave an empty first part and produce `RRULE:;UNTIL=`.
    kept.retain(|p| !p.is_empty());
    kept.push(format!("UNTIL={}", until_value(before_ms, is_all_day, tz)));

    format!("{prefix}{}", kept.join(";"))
}

/// Whether `line` is the `RRULE` of a `recurrence` array.
///
/// Google's `recurrence` is a list of iCalendar *lines*, only one of which is
/// the repeat rule — the others are `EXDATE`/`RDATE` lists naming individual
/// occurrences that were removed from or added to the series. Only the `RRULE`
/// is truncated; the rest are carried across a split verbatim, since an
/// `EXDATE` in the tail is an occurrence somebody deleted and re-creating it
/// would resurrect a meeting that was cancelled.
pub(crate) fn is_rrule(line: &str) -> bool {
    line.trim_start().len() >= 5 && line.trim_start()[..5].eq_ignore_ascii_case("RRULE")
}

/// Whether `rule` ends after a fixed number of occurrences rather than on a
/// date. Splitting one of these correctly means working out how many
/// occurrences the first half consumed, which `events::split_series` refuses
/// to guess at — see its doc comment.
pub(crate) fn has_count(rule: &str) -> bool {
    let parts = rule.split_once(':').map_or(rule, |(_, p)| p);
    parts
        .split(';')
        .any(|part| part.split('=').next().unwrap_or_default().eq_ignore_ascii_case("COUNT"))
}

/// Which Repeat option, if any, represents `rule` exactly.
///
/// Exact string equality against what [`rrule_for`] authors, deliberately —
/// not a parse. A rule carrying `INTERVAL`, `COUNT`, `UNTIL`, `EXDATE` or a
/// `BYDAY` we did not write is `custom`, and the UI must then refuse to
/// overwrite it. Being generous here (parsing `FREQ` and ignoring the rest)
/// is exactly how "every 2nd Tuesday" becomes "weekly" behind the user's back.
///
/// Called once, on the read side: `events::event_detail_impl` puts the answer
/// on `EventDetail::repeat` so the Repeat control has it without re-deriving
/// it. No write command needs it — a write is told which option the user
/// picked and maps it forward through [`rrule_for`].
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

    #[test]
    fn an_all_day_when_needs_no_zone_and_sends_bare_dates() {
        let w = When::AllDay { start_date: "2026-08-10".into(), end_date: "2026-08-11".into() };
        // The zone is deliberately absurd: an all-day event must not consult it.
        let (start, end) = when_json(&w, "Not/AZone");
        assert_eq!(start, serde_json::json!({ "date": "2026-08-10" }));
        assert_eq!(end, serde_json::json!({ "date": "2026-08-11" }));
        assert!(start.get("dateTime").is_none());
        assert!(start.get("timeZone").is_none());
    }

    #[test]
    fn a_timed_when_sends_datetime_and_zone() {
        let w = When::Timed { start_ms: 1_785_398_400_000, end_ms: 1_785_402_000_000 };
        let (start, end) = when_json(&w, "Europe/Sofia");
        assert!(start["dateTime"].is_string());
        assert_eq!(start["timeZone"], "Europe/Sofia");
        assert!(start.get("date").is_none());
        assert!(end["dateTime"].is_string());
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

    /// 2026-07-30T08:00:00Z. Verify with:
    /// `python3 -c "import datetime as d; print(d.datetime.fromtimestamp(1785398400, d.timezone.utc))"`
    /// — one second earlier is 07:59:59Z, which is what every `UNTIL` below
    /// expects. An earlier draft of the plan said `20260731` and was a day out.
    const SPLIT: i64 = 1_785_398_400_000;

    /// UNTIL is inclusive in RFC 5545, so it must land strictly before the
    /// occurrence that moves to the new series — one second earlier. Getting
    /// this wrong duplicates that occurrence in both series or drops it from
    /// both.
    #[test]
    fn until_lands_one_second_before_the_split() {
        let r = truncated_rule("RRULE:FREQ=WEEKLY", SPLIT, false, "UTC");
        assert_eq!(r, "RRULE:FREQ=WEEKLY;UNTIL=20260730T075959Z");
    }

    /// An existing UNTIL is replaced, not appended — two UNTILs is invalid.
    #[test]
    fn an_existing_until_is_replaced() {
        let r = truncated_rule("RRULE:FREQ=WEEKLY;UNTIL=20271231T000000Z", SPLIT, false, "UTC");
        assert_eq!(r.matches("UNTIL").count(), 1);
        assert!(r.ends_with("UNTIL=20260730T075959Z"), "got {r}");
    }

    /// COUNT and UNTIL are mutually exclusive in RFC 5545; a rule carrying
    /// COUNT must lose it when truncated.
    #[test]
    fn count_is_dropped_when_until_is_added() {
        let r = truncated_rule("RRULE:FREQ=DAILY;COUNT=10", SPLIT, false, "UTC");
        assert!(!r.contains("COUNT"), "got {r}");
        assert!(r.contains("UNTIL="));
    }

    /// The parts this app cannot author are exactly the ones a truncation must
    /// not disturb: "every second Tuesday and Thursday" is carried through in
    /// its original order, with only the ending rewritten. Rebuilding the rule
    /// from a parse, or reordering it, is how a fortnightly meeting quietly
    /// becomes a weekly one.
    #[test]
    fn every_other_part_survives_truncation_in_its_original_order() {
        let r = truncated_rule(
            "RRULE:FREQ=WEEKLY;INTERVAL=2;BYDAY=TU,TH;WKST=SU;COUNT=8",
            SPLIT,
            false,
            "UTC",
        );
        assert_eq!(r, "RRULE:FREQ=WEEKLY;INTERVAL=2;BYDAY=TU,TH;WKST=SU;UNTIL=20260730T075959Z");
    }

    /// The timed form is an absolute UTC instant on both sides, so no zone the
    /// caller passes can move it — which is what makes "no DST hazard" a
    /// property rather than a hope. `Pacific/Kiritimati` (UTC+14) and
    /// `Pacific/Niue` (UTC-11) are 25 hours apart and would land on different
    /// *dates* if the zone were consulted at all.
    #[test]
    fn a_timed_until_is_the_same_instant_whatever_zone_it_is_asked_for() {
        let utc = truncated_rule("RRULE:FREQ=WEEKLY", SPLIT, false, "UTC");
        for tz in ["Pacific/Kiritimati", "Pacific/Niue", "America/New_York", "Not/AZone"] {
            assert_eq!(truncated_rule("RRULE:FREQ=WEEKLY", SPLIT, false, tz), utc, "{tz}");
        }
    }

    /// RFC 5545 §3.3.10: UNTIL must carry the same value type as DTSTART. An
    /// all-day series' DTSTART is a bare date, so its UNTIL must be one too —
    /// and a date-valued UNTIL is inclusive of the whole day it names, so
    /// "before this occurrence" is the *previous day*, not the previous
    /// second. Emitting `20260730T075959Z` here produces a rule other clients
    /// reject, and one whose last day is ambiguous even where they don't.
    #[test]
    fn an_all_day_until_is_a_bare_date_on_the_previous_day() {
        // Midnight on 2026-07-30 in Sofia, the instant an all-day occurrence
        // on that date is stored at.
        let midnight = "2026-07-30T00:00:00"
            .parse::<jiff::civil::DateTime>()
            .unwrap()
            .in_tz("Europe/Sofia")
            .unwrap()
            .timestamp()
            .as_millisecond();
        let r = truncated_rule("RRULE:FREQ=WEEKLY", midnight, true, "Europe/Sofia");
        assert_eq!(r, "RRULE:FREQ=WEEKLY;UNTIL=20260729");
    }

    /// The all-day date is read in the zone it is *stored* against, the same
    /// one `event_time_json` renders the `start` of the very same write in.
    /// Reading it in UTC instead is a day out for every zone whose midnight
    /// falls on the other side of it — `Pacific/Auckland` (UTC+12) is midday
    /// the *previous* day in UTC, so the series would keep an occurrence the
    /// user split away.
    #[test]
    fn an_all_day_until_is_read_in_the_events_own_zone_not_utc() {
        let midnight = "2026-07-30T00:00:00"
            .parse::<jiff::civil::DateTime>()
            .unwrap()
            .in_tz("Pacific/Auckland")
            .unwrap()
            .timestamp()
            .as_millisecond();
        assert_eq!(
            jiff::Timestamp::from_millisecond(midnight).unwrap().to_string(),
            "2026-07-29T12:00:00Z",
            "fixture check: this instant must fall on the previous date in UTC, or the \
             assertion below proves nothing"
        );
        let r = truncated_rule("RRULE:FREQ=WEEKLY", midnight, true, "Pacific/Auckland");
        assert_eq!(r, "RRULE:FREQ=WEEKLY;UNTIL=20260729");
    }

    /// Google always emits uppercase, but a rule that arrived through another
    /// client is not ours to trust: a lowercase `count=` left in place is a
    /// rule carrying both COUNT and UNTIL, which RFC 5545 forbids outright.
    #[test]
    fn a_lowercase_count_or_until_is_still_replaced() {
        let r = truncated_rule("RRULE:freq=DAILY;count=10;until=20271231T000000Z", SPLIT, false, "UTC");
        assert_eq!(r, "RRULE:freq=DAILY;UNTIL=20260730T075959Z");
    }

    #[test]
    fn an_rrule_is_told_from_the_other_recurrence_lines() {
        assert!(is_rrule("RRULE:FREQ=WEEKLY"));
        assert!(is_rrule("rrule:FREQ=WEEKLY"));
        assert!(!is_rrule("EXDATE;TZID=Europe/Sofia:20260810T090000"));
        assert!(!is_rrule("RDATE:20260810T090000Z"));
        assert!(!is_rrule(""));
    }

    #[test]
    fn a_count_rule_is_told_from_a_dated_one() {
        assert!(has_count("RRULE:FREQ=DAILY;COUNT=10"));
        assert!(has_count("RRULE:FREQ=DAILY;count=10"));
        assert!(!has_count("RRULE:FREQ=DAILY"));
        assert!(!has_count("RRULE:FREQ=DAILY;UNTIL=20271231T000000Z"));
        // Not a substring match: `BYSETPOS` and friends must not be mistaken
        // for a `COUNT`, and a value that happens to contain the word is not a
        // key. Both would refuse a split that is perfectly safe.
        assert!(!has_count("RRULE:FREQ=MONTHLY;BYDAY=MO;BYSETPOS=-1"));
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
