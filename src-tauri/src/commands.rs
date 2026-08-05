use omacal_core::{expand, lay_out_day, pack_lanes, Interval, Lane, Placed, Segment, Series};
use omacal_store::StoredEvent;
use serde::Serialize;

const DAY_MS: i64 = 24 * 3_600_000;
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
    pub guests: u32,
    pub is_all_day: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DayColumn {
    pub start_ms: i64,
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
        color: "#5b8def".into(), // replaced by the calendar's colour in Task 13
        response: src.self_response.clone().unwrap_or_else(|| "accepted".into()),
        guests: 0,
        is_all_day: src.is_all_day,
    }
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

    for src in events {
        for iv in occurrences(src, bounds[0], week_end_ms) {
            if src.is_all_day {
                let start_col = signed_column(&bounds, iv.start_ms);
                // Google's all-day end is exclusive, so the last covered day is
                // one millisecond before it.
                let end_col = signed_column(&bounds, iv.end_ms - 1);
                segments.push(Segment { idx: all_day_events.len(), start_col, end_col });
                all_day_events.push(to_ui(src, iv.start_ms, iv.end_ms));
            } else if let Some(col) = column_for(&bounds, iv.start_ms) {
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
            DayColumn { start_ms: bounds[d], events: evs, placed }
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
            is_all_day: all_day, recurrence: None, status: "confirmed".into(),
            self_response: Some("accepted".into()), conference_uri: None,
        }
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
}
