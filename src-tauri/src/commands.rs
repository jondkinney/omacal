use omacal_core::{expand, lay_out_day, pack_lanes, Interval, Lane, Placed, Segment, Series};
use omacal_store::StoredEvent;
use serde::Serialize;
use std::collections::HashSet;

const DAY_MS: i64 = 24 * 3_600_000;
/// Used when a calendar carries no colour of its own — Google omits
/// `backgroundColor` on some calendars, and a missing colour must not be a
/// missing event.
const DEFAULT_EVENT_COLOR: &str = "#5b8def";
/// Expansion guard for one week of any single series. Sized for the realistic
/// worst case — a 30-minute block recurring through every working hour is ~336
/// occurrences a week — so that `Expansion::truncated` stays false in practice.
const EXPAND_LIMIT: u16 = 512;

#[derive(Debug, Clone, Serialize)]
pub struct UiEvent {
    pub id: i64,
    pub title: String,
    pub location: Option<String>,
    pub start_ms: i64,
    pub end_ms: i64,
    pub color: String,
    /// `accepted` | `needsAction` | `tentative` | `declined`
    pub response: String,
    pub is_all_day: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DayColumn {
    pub start_ms: i64,
    /// Midnight on the *next* day, in the display zone. Carried explicitly so
    /// the UI can draw against the column's true span: a DST day is 23 or 25
    /// hours long, and `start_ms + 24h` puts every hour rule an hour out.
    pub end_ms: i64,
    pub events: Vec<UiEvent>,
    pub placed: Vec<Placed>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WeekPayload {
    pub days: Vec<DayColumn>,
    pub all_day: Vec<Lane>,
    pub all_day_events: Vec<UiEvent>,
    pub overflow: Vec<usize>,
}

fn to_ui(src: &StoredEvent, start_ms: i64, end_ms: i64) -> UiEvent {
    UiEvent {
        id: src.id,
        title: src.summary.clone().unwrap_or_else(|| "(no title)".into()),
        location: src.location.clone(),
        start_ms,
        end_ms,
        color: src
            .color_hex
            .clone()
            .unwrap_or_else(|| DEFAULT_EVENT_COLOR.into()),
        response: src.self_response.clone().unwrap_or_else(|| "accepted".into()),
        is_all_day: src.is_all_day,
    }
}

/// The occurrences that an exception has taken over from its master.
///
/// An exception is stored as a standalone row carrying the id of the series it
/// overrides and the instant the overridden occurrence started at. Both a moved
/// instance and a deleted one produce such a row, and in both cases the master
/// must stop expanding into that slot — otherwise a moved instance renders
/// twice (once at its new time, once as a ghost at the old one) and a deleted
/// one never disappears at all.
fn suppressed_slots(events: &[StoredEvent]) -> HashSet<(i64, &str, i64)> {
    events
        .iter()
        .filter_map(|e| {
            let master = e.recurring_event_id.as_deref()?;
            Some((e.calendar_id, master, e.original_start_utc?))
        })
        .collect()
}

/// Expands one stored row into the concrete occurrences overlapping the window.
fn occurrences(src: &StoredEvent, from_ms: i64, to_ms: i64) -> Vec<Interval> {
    let Some(rule) = &src.recurrence else {
        let iv = Interval { start_ms: src.start_utc, end_ms: src.end_utc };
        return if iv.start_ms < to_ms && iv.end_ms > from_ms { vec![iv] } else { vec![] };
    };
    let lines: Vec<String> = rule.lines().map(|s| s.to_string()).collect();
    let series = Series {
        dtstart_ms: src.start_utc,
        dtstart_tz: &src.start_tz,
        duration_ms: src.end_utc - src.start_utc,
        is_all_day: src.is_all_day,
        recurrence: &lines,
    };
    match expand(&series, from_ms, to_ms, EXPAND_LIMIT) {
        Ok(e) => {
            if e.truncated {
                // Surfaced rather than swallowed: a series this dense means the
                // window is showing an incomplete picture.
                tracing::warn!(
                    google_id = %src.google_id,
                    limit = EXPAND_LIMIT,
                    "recurrence expansion truncated"
                );
            }
            e.intervals
        }
        Err(err) => {
            tracing::warn!(google_id = %src.google_id, %err, "recurrence expansion failed");
            Vec::new()
        }
    }
}

/// The eight instants bounding the week's seven days, computed **in `tz`**.
///
/// Never `week_start + n * DAY_MS`: on a DST transition day that arithmetic is
/// off by an hour, which both misplaces events and squashes or stretches the
/// day's geometry. `Zoned::checked_add(1.day())` does calendar-day arithmetic,
/// so a 23- or 25-hour day comes out at its true length.
///
/// Falls back to fixed 24-hour days only if the zone is unknown — the grid must
/// still render.
fn day_boundaries(week_start_ms: i64, tz: &str) -> Vec<i64> {
    use jiff::{Timestamp, ToSpan};

    let fallback = || (0..=7).map(|i| week_start_ms + i * DAY_MS).collect::<Vec<_>>();

    let Ok(start) = Timestamp::from_millisecond(week_start_ms) else {
        return fallback();
    };
    let Ok(mut z) = start.in_tz(tz) else {
        return fallback();
    };

    let mut out = Vec::with_capacity(8);
    out.push(z.timestamp().as_millisecond());
    for _ in 0..7 {
        match z.checked_add(1.day()) {
            Ok(next) => {
                z = next;
                out.push(z.timestamp().as_millisecond());
            }
            Err(_) => {
                let last = *out.last().unwrap();
                out.push(last + DAY_MS);
            }
        }
    }
    out
}

/// Column `0..=6` for an instant inside the week, or `None` outside it.
fn column_for(bounds: &[i64], ms: i64) -> Option<usize> {
    if ms < bounds[0] || ms >= bounds[7] {
        return None;
    }
    Some(bounds.partition_point(|&b| b <= ms) - 1)
}

/// The column a timed occurrence should be drawn in, or `None` if it does not
/// touch the week at all.
///
/// Normally that is the column containing its start. An event that began before
/// the week and runs into it — Sunday 23:00 to Monday 01:00, on the Monday the
/// week begins — has no such column, and dropping it made the event vanish
/// entirely. It is clamped into column 0 instead, where `lay_out_day` clips the
/// geometry to the column's own bounds.
fn timed_column(bounds: &[i64], iv: &Interval) -> Option<usize> {
    column_for(bounds, iv.start_ms)
        .or_else(|| (iv.start_ms < bounds[0] && iv.end_ms > bounds[0]).then_some(0))
}

/// Column index that may fall outside `0..=6`, for all-day spans that begin
/// before or end after this week. Only the sign matters to `pack_lanes`, which
/// turns it into a continuation flag, so approximating the days beyond the
/// week's edges with fixed 24-hour arithmetic is harmless.
fn signed_column(bounds: &[i64], ms: i64) -> i32 {
    if ms < bounds[0] {
        -(((bounds[0] - ms - 1) / DAY_MS) + 1) as i32
    } else if ms >= bounds[7] {
        7 + ((ms - bounds[7]) / DAY_MS) as i32
    } else {
        (bounds.partition_point(|&b| b <= ms) - 1) as i32
    }
}

/// Turns stored events into seven laid-out day columns plus the all-day band.
///
/// `week_start_ms` is midnight in `tz` on the week's Monday; `tz` is the display
/// zone. All day-boundary maths flows through `day_boundaries`, so a week
/// containing a DST transition lays out correctly.
pub fn assemble_week(events: &[StoredEvent], week_start_ms: i64, tz: &str) -> WeekPayload {
    let bounds = day_boundaries(week_start_ms, tz);
    let week_end_ms = bounds[7];

    let mut day_events: Vec<Vec<UiEvent>> = vec![Vec::new(); 7];
    let mut all_day_events: Vec<UiEvent> = Vec::new();
    let mut segments: Vec<Segment> = Vec::new();

    let suppressed = suppressed_slots(events);

    for src in events {
        // A cancelled exception exists only to record that an occurrence was
        // deleted. It has already been counted into `suppressed`; it draws
        // nothing itself.
        if src.status == "cancelled" {
            continue;
        }
        for iv in occurrences(src, bounds[0], week_end_ms) {
            // Only a master can match: the keys are master ids, and an
            // exception never carries its own id as its master.
            if suppressed.contains(&(src.calendar_id, src.google_id.as_str(), iv.start_ms)) {
                continue;
            }
            if src.is_all_day {
                let start_col = signed_column(&bounds, iv.start_ms);
                // Google's all-day end is exclusive, so the last covered day is
                // one millisecond before it.
                let end_col = signed_column(&bounds, iv.end_ms - 1);
                segments.push(Segment { idx: all_day_events.len(), start_col, end_col });
                all_day_events.push(to_ui(src, iv.start_ms, iv.end_ms));
            } else if let Some(col) = timed_column(&bounds, &iv) {
                day_events[col].push(to_ui(src, iv.start_ms, iv.end_ms));
            }
        }
    }

    let (all_day, overflow) = pack_lanes(&segments, 7, 2);

    let days = (0..7)
        .map(|d| {
            let evs = std::mem::take(&mut day_events[d]);
            let intervals: Vec<Interval> = evs
                .iter()
                .map(|e| Interval { start_ms: e.start_ms, end_ms: e.end_ms })
                .collect();
            // The window is the day's *true* length, so a 25-hour day is not
            // compressed into 24 hours' worth of geometry.
            let placed = lay_out_day(&intervals, bounds[d], bounds[d + 1]);
            DayColumn { start_ms: bounds[d], end_ms: bounds[d + 1], events: evs, placed }
        })
        .collect();

    WeekPayload { days, all_day, all_day_events, overflow }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(gid: &str, start: i64, end: i64, all_day: bool) -> omacal_store::StoredEvent {
        omacal_store::StoredEvent {
            id: 0, calendar_id: 1, google_id: gid.into(), summary: Some(gid.into()),
            location: None, start_utc: start, end_utc: end,
            start_tz: "UTC".into(), end_tz: "UTC".into(),
            is_all_day: all_day, recurrence: None,
            recurring_event_id: None, original_start_utc: None,
            status: "confirmed".into(),
            self_response: Some("accepted".into()), conference_uri: None,
            color_hex: None,
            description: None, etag: None, sequence: 0, organizer_email: None,
            attendees: Vec::new(),
        }
    }

    /// A daily 09:00–09:30 series starting on the week's Monday.
    fn daily_master() -> omacal_store::StoredEvent {
        let mut m = ev("standup", MON + 9 * 3_600_000, MON + 9 * 3_600_000 + 1_800_000, false);
        m.recurrence = Some("RRULE:FREQ=DAILY".into());
        m
    }

    const DAY: i64 = 24 * 3_600_000;
    /// Monday 2026-08-03 00:00:00 UTC
    const MON: i64 = 1_785_715_200_000;

    #[test]
    fn a_timed_event_lands_in_its_own_day_column() {
        let evs = vec![ev("a", MON + 9 * 3_600_000, MON + 10 * 3_600_000, false)];
        let w = assemble_week(&evs, MON, "UTC");
        assert_eq!(w.days[0].events.len(), 1);
        assert!(w.days[1].events.is_empty());
    }

    #[test]
    fn an_event_on_wednesday_lands_in_column_two() {
        let evs = vec![ev("a", MON + 2 * DAY + 9 * 3_600_000, MON + 2 * DAY + 10 * 3_600_000, false)];
        let w = assemble_week(&evs, MON, "UTC");
        assert_eq!(w.days[2].events.len(), 1);
    }

    #[test]
    fn overlapping_events_get_two_columns() {
        let evs = vec![
            ev("a", MON + 10 * 3_600_000, MON + 11 * 3_600_000, false),
            ev("b", MON + 10 * 3_600_000, MON + 11 * 3_600_000, false),
        ];
        let w = assemble_week(&evs, MON, "UTC");
        assert_eq!(w.days[0].placed[0].columns, 2);
    }

    #[test]
    fn all_day_events_go_to_the_band_not_the_grid() {
        let evs = vec![ev("trip", MON, MON + 3 * DAY, true)];
        let w = assemble_week(&evs, MON, "UTC");
        assert!(w.days[0].events.is_empty());
        assert_eq!(w.all_day_events.len(), 1);
        assert_eq!(w.all_day[0].start_col, 0);
        assert_eq!(w.all_day[0].end_col, 2);
    }

    #[test]
    fn an_all_day_span_entering_the_week_is_flagged_as_continuing() {
        let evs = vec![ev("trip", MON - 3 * DAY, MON + 2 * DAY, true)];
        let w = assemble_week(&evs, MON, "UTC");
        assert!(w.all_day[0].cont_left);
        assert!(!w.all_day[0].cont_right);
    }

    #[test]
    fn events_outside_the_week_are_dropped() {
        let evs = vec![ev("a", MON + 30 * DAY, MON + 30 * DAY + 3_600_000, false)];
        let w = assemble_week(&evs, MON, "UTC");
        assert!(w.days.iter().all(|d| d.events.is_empty()));
    }

    #[test]
    fn a_recurring_master_is_expanded_across_the_week() {
        let mut master = ev("standup", MON + 9 * 3_600_000, MON + 9 * 3_600_000 + 1_800_000, false);
        master.recurrence = Some("RRULE:FREQ=DAILY".into());
        let w = assemble_week(&[master], MON, "UTC");
        let total: usize = w.days.iter().map(|d| d.events.len()).sum();
        assert_eq!(total, 7);
    }

    /// Monday 2026-10-19 00:00 in Europe/Sofia. That week contains the
    /// end of DST on Sunday 2026-10-25, making that Sunday 25 hours long.
    fn dst_week_start() -> i64 {
        jiff::civil::date(2026, 10, 19)
            .at(0, 0, 0, 0)
            .in_tz("Europe/Sofia")
            .unwrap()
            .timestamp()
            .as_millisecond()
    }

    #[test]
    fn a_dst_week_contains_a_twenty_five_hour_day() {
        let bounds = day_boundaries(dst_week_start(), "Europe/Sofia");
        let lengths: Vec<i64> = bounds.windows(2).map(|w| w[1] - w[0]).collect();
        assert_eq!(lengths.len(), 7);
        assert!(
            lengths.contains(&(25 * 3_600_000)),
            "expected one 25-hour day, got {lengths:?}"
        );
    }

    #[test]
    fn a_normal_week_has_seven_equal_days() {
        let bounds = day_boundaries(dst_week_start() - 7 * DAY, "Europe/Sofia");
        let lengths: Vec<i64> = bounds.windows(2).map(|w| w[1] - w[0]).collect();
        assert!(lengths.iter().all(|&l| l == DAY), "got {lengths:?}");
    }

    #[test]
    fn an_event_late_on_a_long_day_stays_inside_its_column() {
        let start = dst_week_start();
        let bounds = day_boundaries(start, "Europe/Sofia");
        // 24h30m into the 25-hour Sunday: valid, and only representable
        // because the day window is its true length.
        let late = bounds[6] + 24 * 3_600_000 + 1_800_000;
        assert_eq!(column_for(&bounds, late), Some(6));

        let w = assemble_week(&[ev("night", late, late + 1_800_000, false)], start, "Europe/Sofia");
        assert_eq!(w.days[6].events.len(), 1);
        let p = w.days[6].placed[0];
        assert!(p.top < 1.0, "top {} should stay within the day", p.top);
        assert!(p.top + p.height <= 1.0001, "block overflows the column");
    }

    #[test]
    fn an_unknown_timezone_still_produces_seven_days() {
        let bounds = day_boundaries(MON, "Mars/Olympus_Mons");
        assert_eq!(bounds.len(), 8);
        assert_eq!(bounds[7] - bounds[0], 7 * DAY);
    }

    /// A moved instance: the master must stop expanding into the slot the
    /// instance came from. Without suppression Tuesday shows two events — the
    /// real one at 14:00 and a ghost at 09:00.
    #[test]
    fn a_moved_instance_replaces_its_original_occurrence() {
        let mut moved = ev("standup_20260804", MON + DAY + 14 * 3_600_000,
                           MON + DAY + 14 * 3_600_000 + 1_800_000, false);
        moved.recurring_event_id = Some("standup".into());
        moved.original_start_utc = Some(MON + DAY + 9 * 3_600_000);

        let w = assemble_week(&[daily_master(), moved], MON, "UTC");

        assert_eq!(w.days[1].events.len(), 1, "Tuesday must show one event, not two");
        assert_eq!(w.days[1].events[0].start_ms, MON + DAY + 14 * 3_600_000);
        // Every other day keeps its ordinary occurrence.
        let total: usize = w.days.iter().map(|d| d.events.len()).sum();
        assert_eq!(total, 7);
    }

    /// A deleted instance: Google sends a cancelled exception. It is stored,
    /// renders nothing itself, and silences the master for that one day.
    #[test]
    fn a_cancelled_instance_empties_its_day_and_no_other() {
        let mut cancelled = ev("standup_20260805", MON + 2 * DAY + 9 * 3_600_000,
                               MON + 2 * DAY + 9 * 3_600_000, false);
        cancelled.status = "cancelled".into();
        cancelled.recurring_event_id = Some("standup".into());
        cancelled.original_start_utc = Some(MON + 2 * DAY + 9 * 3_600_000);

        let w = assemble_week(&[daily_master(), cancelled], MON, "UTC");

        assert!(w.days[2].events.is_empty(), "the deleted occurrence must be gone");
        assert_eq!(w.days[1].events.len(), 1, "the day before is unaffected");
        assert_eq!(w.days[3].events.len(), 1, "the day after is unaffected");
        let total: usize = w.days.iter().map(|d| d.events.len()).sum();
        assert_eq!(total, 6);
    }

    /// An instance dragged clean out of the week still has to silence the slot
    /// it left behind, so the store returns it and nothing renders on that day.
    #[test]
    fn an_instance_moved_out_of_the_week_leaves_no_ghost() {
        let mut moved = ev("standup_20260806", MON + 40 * DAY, MON + 40 * DAY + 1_800_000, false);
        moved.recurring_event_id = Some("standup".into());
        moved.original_start_utc = Some(MON + 3 * DAY + 9 * 3_600_000);

        let w = assemble_week(&[daily_master(), moved], MON, "UTC");
        assert!(w.days[3].events.is_empty());
        let total: usize = w.days.iter().map(|d| d.events.len()).sum();
        assert_eq!(total, 6);
    }

    /// An exception only silences its own master, and only on its own calendar.
    #[test]
    fn an_exception_does_not_silence_an_unrelated_series() {
        let mut other = ev("other_ex", MON + DAY + 14 * 3_600_000,
                           MON + DAY + 15 * 3_600_000, false);
        other.recurring_event_id = Some("some-other-series".into());
        other.original_start_utc = Some(MON + DAY + 9 * 3_600_000);

        let w = assemble_week(&[daily_master(), other], MON, "UTC");
        // Tuesday keeps its standup *and* gains the unrelated exception.
        assert_eq!(w.days[1].events.len(), 2);
    }

    /// A meeting that runs Sunday 23:00 into Monday 01:00 overlaps the week but
    /// starts before it. Dropping it made it vanish; it belongs in column 0,
    /// clipped to the column.
    #[test]
    fn a_timed_event_starting_before_the_week_is_clamped_into_the_first_column() {
        let evs = vec![ev("night", MON - 3_600_000, MON + 3_600_000, false)];
        let w = assemble_week(&evs, MON, "UTC");
        assert_eq!(w.days[0].events.len(), 1);
        let p = w.days[0].placed[0];
        assert!((p.top - 0.0).abs() < 1e-6, "top {} should clamp to the column start", p.top);
        assert!(p.height > 0.0);
    }

    /// The same shape at the other edge: it starts inside the week and runs out
    /// of it. The geometry must stay inside the last column.
    #[test]
    fn a_timed_event_running_past_the_week_stays_in_the_last_column() {
        let start = MON + 6 * DAY + 23 * 3_600_000;
        let evs = vec![ev("night", start, start + 2 * 3_600_000, false)];
        let w = assemble_week(&evs, MON, "UTC");
        assert_eq!(w.days[6].events.len(), 1);
        let p = w.days[6].placed[0];
        assert!(p.top + p.height <= 1.0001, "block overflows the column");
    }

    /// An event that ends before the week begins still has no column.
    #[test]
    fn an_event_entirely_before_the_week_is_still_dropped() {
        let evs = vec![ev("old", MON - 5 * 3_600_000, MON - 4 * 3_600_000, false)];
        let w = assemble_week(&evs, MON, "UTC");
        assert!(w.days.iter().all(|d| d.events.is_empty()));
    }

    #[test]
    fn an_event_carries_its_calendars_colour() {
        let mut e = ev("a", MON + 9 * 3_600_000, MON + 10 * 3_600_000, false);
        e.color_hex = Some("#b58900".into());
        let w = assemble_week(&[e], MON, "UTC");
        assert_eq!(w.days[0].events[0].color, "#b58900");
    }

    #[test]
    fn a_calendar_without_a_colour_falls_back_to_the_default() {
        let evs = vec![ev("a", MON + 9 * 3_600_000, MON + 10 * 3_600_000, false)];
        let w = assemble_week(&evs, MON, "UTC");
        assert_eq!(w.days[0].events[0].color, DEFAULT_EVENT_COLOR);
    }

    /// The UI draws hour rules and the now-line against `end_ms - start_ms`, so
    /// a long day has to report its true length rather than a nominal 24 hours.
    #[test]
    fn each_column_reports_its_true_span() {
        let w = assemble_week(&[], dst_week_start(), "Europe/Sofia");
        let spans: Vec<i64> = w.days.iter().map(|d| d.end_ms - d.start_ms).collect();
        assert!(spans.contains(&(25 * 3_600_000)), "expected a 25-hour day, got {spans:?}");
        // Each column ends exactly where the next begins; no gaps, no overlaps.
        for pair in w.days.windows(2) {
            assert_eq!(pair[0].end_ms, pair[1].start_ms);
        }
    }
}
