//! Pure builders for event write bodies.
//!
//! Everything here is a function of its arguments: no pool, no client, no
//! clock. The write commands stay thin wrappers around these so the rules
//! that matter — "never send a field the user did not touch", "all-day means
//! `date` not `dateTime`" — are testable without a server.

// The write commands that call these land in a later task; until then
// they're exercised only by their own tests below.
#![allow(dead_code)]

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

/// A PATCH body carrying only what actually changed.
///
/// A field absent from a PATCH body means "leave it alone"; a field present
/// and null means "clear it". Both are needed, and conflating them makes
/// clearing a location impossible.
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
