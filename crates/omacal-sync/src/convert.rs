use jiff::civil::Date;
use jiff::Timestamp;
use omacal_google::model::{Event, EventDateTime};
use omacal_store::StoredEvent;

/// Incremental sync delivers deletions as `status: "cancelled"` rows that carry
/// little more than an id.
pub fn is_tombstone(ev: &Event) -> bool {
    ev.status == "cancelled"
}

/// Resolves one endpoint to an epoch-millisecond instant.
///
/// Timed events carry RFC 3339 with an offset. All-day events carry a bare
/// date, which must be interpreted in the calendar's zone — midnight in Sofia
/// is not midnight UTC.
fn resolve(dt: &EventDateTime, cal_tz: &str) -> Option<i64> {
    if let Some(s) = &dt.date_time {
        return s.parse::<Timestamp>().ok().map(|t| t.as_millisecond());
    }
    let d = dt.date.as_ref()?;
    let date: Date = d.parse().ok()?;
    let tz = dt.time_zone.as_deref().unwrap_or(cal_tz);
    date.to_datetime(jiff::civil::Time::midnight())
        .in_tz(tz)
        .ok()
        .map(|z| z.timestamp().as_millisecond())
}

/// Converts a wire event into a storable row. Returns `None` for tombstones and
/// for rows whose times cannot be parsed — a malformed event must not abort a
/// whole sync page.
pub fn to_stored(ev: &Event, calendar_id: i64, cal_tz: &str) -> Option<StoredEvent> {
    if is_tombstone(ev) {
        return None;
    }
    let start_utc = resolve(&ev.start, cal_tz)?;
    let end_utc = resolve(&ev.end, cal_tz)?;
    let is_all_day = ev.start.date.is_some();

    Some(StoredEvent {
        id: 0,
        calendar_id,
        google_id: ev.id.clone(),
        summary: ev.summary.clone(),
        location: ev.location.clone(),
        start_utc,
        end_utc,
        start_tz: ev
            .start
            .time_zone
            .clone()
            .unwrap_or_else(|| cal_tz.to_string()),
        // Kept separately from `start_tz`: a flight departs in one zone and
        // lands in another, and collapsing the two loses that.
        end_tz: ev
            .end
            .time_zone
            .clone()
            .or_else(|| ev.start.time_zone.clone())
            .unwrap_or_else(|| cal_tz.to_string()),
        is_all_day,
        recurrence: ev.recurrence.as_ref().map(|r| r.join("\n")),
        status: ev.status.clone(),
        self_response: ev
            .attendees
            .iter()
            .find(|a| a.is_self)
            .map(|a| a.response_status.clone()),
        conference_uri: ev.hangout_link.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use omacal_google::model::{Event, EventDateTime};

    fn timed(start: &str, end: &str) -> Event {
        Event {
            id: "e1".into(), status: "confirmed".into(), etag: None, ical_uid: None,
            summary: Some("Standup".into()), description: None, location: Some("Meet".into()),
            start: EventDateTime { date_time: Some(start.into()), date: None,
                                   time_zone: Some("Europe/Sofia".into()) },
            end: EventDateTime { date_time: Some(end.into()), date: None,
                                 time_zone: Some("Europe/Sofia".into()) },
            recurrence: None, recurring_event_id: None, original_start_time: None,
            hangout_link: None, attendees: vec![], sequence: 0,
        }
    }

    #[test]
    fn a_timed_event_converts_to_utc_millis() {
        let ev = timed("2026-08-03T09:00:00+03:00", "2026-08-03T09:30:00+03:00");
        let s = to_stored(&ev, 1, "Europe/Sofia").unwrap();
        assert_eq!(s.start_utc, 1_785_736_800_000);
        assert_eq!(s.end_utc - s.start_utc, 30 * 60_000);
        assert_eq!(s.start_tz, "Europe/Sofia");
        assert!(!s.is_all_day);
    }

    /// A flight departs in one zone and lands in another. Both must survive.
    #[test]
    fn a_cross_timezone_event_keeps_both_zones() {
        let mut ev = timed("2026-08-09T09:00:00+05:30", "2026-08-09T13:00:00+03:00");
        ev.start.time_zone = Some("Asia/Kolkata".into());
        ev.end.time_zone = Some("Europe/Sofia".into());
        let s = to_stored(&ev, 1, "Europe/Sofia").unwrap();
        assert_eq!(s.start_tz, "Asia/Kolkata");
        assert_eq!(s.end_tz, "Europe/Sofia");
        // 09:00 IST is 03:30Z; 13:00 EEST is 10:00Z.
        assert_eq!(s.end_utc - s.start_utc, 6 * 3_600_000 + 1_800_000);
    }

    #[test]
    fn end_zone_defaults_to_the_start_zone_when_absent() {
        let mut ev = timed("2026-08-03T09:00:00+03:00", "2026-08-03T09:30:00+03:00");
        ev.end.time_zone = None;
        let s = to_stored(&ev, 1, "UTC").unwrap();
        assert_eq!(s.end_tz, "Europe/Sofia");
    }

    #[test]
    fn an_all_day_event_uses_the_calendar_timezone() {
        let mut ev = timed("", "");
        ev.start = EventDateTime { date: Some("2026-08-08".into()), ..Default::default() };
        ev.end = EventDateTime { date: Some("2026-08-09".into()), ..Default::default() };
        let s = to_stored(&ev, 1, "Europe/Sofia").unwrap();
        assert!(s.is_all_day);
        // Google's all-day end date is exclusive; one calendar day must remain
        // exactly one day long.
        assert_eq!(s.end_utc - s.start_utc, 24 * 3_600_000);
    }

    #[test]
    fn a_cancelled_event_is_a_tombstone() {
        let mut ev = timed("2026-08-03T09:00:00+03:00", "2026-08-03T09:30:00+03:00");
        ev.status = "cancelled".into();
        assert!(is_tombstone(&ev));
        assert!(to_stored(&ev, 1, "Europe/Sofia").is_none());
    }

    #[test]
    fn recurrence_lines_are_joined_with_newlines() {
        let mut ev = timed("2026-08-03T09:00:00+03:00", "2026-08-03T09:30:00+03:00");
        ev.recurrence = Some(vec!["RRULE:FREQ=DAILY".into(), "EXDATE:20260804T060000Z".into()]);
        let s = to_stored(&ev, 1, "Europe/Sofia").unwrap();
        assert_eq!(s.recurrence.unwrap(), "RRULE:FREQ=DAILY\nEXDATE:20260804T060000Z");
    }

    #[test]
    fn the_self_attendee_response_is_captured() {
        let mut ev = timed("2026-08-03T09:00:00+03:00", "2026-08-03T09:30:00+03:00");
        ev.attendees = vec![
            omacal_google::model::Attendee {
                email: "other@x".into(), display_name: None,
                response_status: "accepted".into(), optional: false, is_self: false },
            omacal_google::model::Attendee {
                email: "me@x".into(), display_name: None,
                response_status: "needsAction".into(), optional: false, is_self: true },
        ];
        let s = to_stored(&ev, 1, "Europe/Sofia").unwrap();
        assert_eq!(s.self_response.as_deref(), Some("needsAction"));
    }

    #[test]
    fn an_unparseable_start_is_skipped_rather_than_panicking() {
        let ev = timed("not-a-date", "also-not-a-date");
        assert!(to_stored(&ev, 1, "Europe/Sofia").is_none());
    }
}
