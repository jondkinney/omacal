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

#[derive(Debug, Clone, Serialize)]
pub struct MonthCell {
    pub start_ms: i64,
    pub end_ms: i64,
    /// False for the leading and trailing days that belong to a neighbouring
    /// month. Drawn dimmed rather than blank so the grid stays rectangular.
    pub in_month: bool,
    /// Timed events for this day, sorted by start. The UI decides how many
    /// fit and renders `+N more` from what it drops — cell height is a
    /// layout question and the backend has no business guessing it.
    pub timed: Vec<UiEvent>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MonthRow {
    pub cells: Vec<MonthCell>, // always 7
    /// Multi-day and all-day events spanning this row, lane-packed at
    /// row_len 7. Indices point into `bar_events`.
    pub bars: Vec<Lane>,
    pub bar_events: Vec<UiEvent>,
    pub bar_overflow: Vec<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MonthPayload {
    pub rows: Vec<MonthRow>, // always 6
    pub year: i32,
    pub month: u32, // 1-12
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

/// The `n + 1` local-midnight boundaries starting at `start_ms`, computed
/// **in `tz`**.
///
/// `n + 1` and not `n`: every consumer needs the *end* of the last day, and a
/// DST day is not 24 hours, so it cannot be derived by addition. Never
/// `start_ms + n * DAY_MS` either: on a DST transition day that arithmetic is
/// off by an hour, which both misplaces events and squashes or stretches the
/// day's geometry. `Zoned::checked_add(1.day())` does calendar-day arithmetic,
/// so a 23- or 25-hour day comes out at its true length.
///
/// Falls back to fixed 24-hour days only if the zone is unknown — the grid must
/// still render.
fn n_day_boundaries(start_ms: i64, n: usize, tz: &str) -> Vec<i64> {
    use jiff::{Timestamp, ToSpan};

    let fallback = || (0..=n as i64).map(|i| start_ms + i * DAY_MS).collect::<Vec<_>>();

    let Ok(start) = Timestamp::from_millisecond(start_ms) else {
        return fallback();
    };
    let Ok(mut z) = start.in_tz(tz) else {
        return fallback();
    };

    let mut out = Vec::with_capacity(n + 1);
    out.push(z.timestamp().as_millisecond());
    for _ in 0..n {
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

/// The eight instants bounding a week's seven days.
///
/// A thin `n = 7` alias over `n_day_boundaries`, kept so the boundary-math
/// tests below — written before Day/Month existed and calling this directly
/// — don't need to thread a day count through. Has no caller outside
/// `#[cfg(test)]`.
#[cfg(test)]
fn day_boundaries(week_start_ms: i64, tz: &str) -> Vec<i64> {
    n_day_boundaries(week_start_ms, 7, tz)
}

/// Local midnight for a civil date, in `tz`, as epoch milliseconds. Falls
/// back to UTC if the zone is unknown — the grid must still render, the same
/// fallback philosophy as `n_day_boundaries`.
fn local_midnight_ms(d: jiff::civil::Date, tz: &str) -> i64 {
    let midnight = d.at(0, 0, 0, 0);
    match midnight.in_tz(tz) {
        Ok(z) => z.timestamp().as_millisecond(),
        Err(_) => midnight
            .to_zoned(jiff::tz::TimeZone::UTC)
            .expect("UTC is always a valid zone")
            .timestamp()
            .as_millisecond(),
    }
}

/// The Monday on or before `year`-`month`-01, at local midnight in `tz` — the
/// anchor for the 42-cell month grid.
fn month_grid_start_ms(year: i32, month: u32, tz: &str) -> i64 {
    use jiff::civil::{date, Weekday};

    let mut grid_start = date(year as i16, month as i8, 1);
    while grid_start.weekday() != Weekday::Monday {
        grid_start = grid_start.yesterday().expect("civil date underflow");
    }
    local_midnight_ms(grid_start, tz)
}

/// Column `0..bounds.len() - 1` for an instant inside the window, or `None`
/// outside it.
fn column_for(bounds: &[i64], ms: i64) -> Option<usize> {
    if ms < bounds[0] || ms >= *bounds.last().unwrap() {
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

/// Column index that may fall outside `0..bounds.len() - 1`, for all-day
/// spans that begin before or end after this window. Only the sign matters to
/// `pack_lanes`, which turns it into a continuation flag, so approximating the
/// days beyond the window's edges with fixed 24-hour arithmetic is harmless.
fn signed_column(bounds: &[i64], ms: i64) -> i32 {
    let n = bounds.len() - 1;
    let end = bounds[n];
    if ms < bounds[0] {
        -(((bounds[0] - ms - 1) / DAY_MS) + 1) as i32
    } else if ms >= end {
        n as i32 + ((ms - end) / DAY_MS) as i32
    } else {
        (bounds.partition_point(|&b| b <= ms) - 1) as i32
    }
}

/// Turns stored events into `n` laid-out day columns plus the all-day band.
///
/// `start_ms` is midnight in `tz` on the window's first day; `tz` is the
/// display zone. All day-boundary maths flows through `n_day_boundaries`, so
/// a window containing a DST transition lays out correctly.
pub fn assemble_days(events: &[StoredEvent], start_ms: i64, n: usize, tz: &str) -> WeekPayload {
    let bounds = n_day_boundaries(start_ms, n, tz);
    let end_ms = bounds[n];

    let mut day_events: Vec<Vec<UiEvent>> = vec![Vec::new(); n];
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
        for iv in occurrences(src, bounds[0], end_ms) {
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

    let (all_day, overflow) = pack_lanes(&segments, n as u16, 2);

    let days = (0..n)
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

/// The week view. A thin wrapper so `assemble_days` has one caller shape and
/// Day view is provably the same engine — see
/// `one_day_and_seven_days_agree_about_the_day_they_share`.
pub fn assemble_week(events: &[StoredEvent], week_start_ms: i64, tz: &str) -> WeekPayload {
    assemble_days(events, week_start_ms, 7, tz)
}

/// Six week-rows of a calendar month, always 42 cells regardless of how many
/// weeks the month actually spans — a grid that changes height as you page
/// through the year is worse than one dimmed row.
///
/// Unlike `assemble_days`, Month needs no time positioning: each row is
/// lane-packed independently at `row_len = 7` for its own spanning bars, and
/// each cell gets a flat, sorted list of the day's timed events for the UI to
/// lay out.
///
/// Not called from `lib.rs` yet — `get_month` is wired up in the next task.
/// Suppressed here rather than left to rot unverified; the tests below are
/// the only current caller. `MonthCell`/`MonthRow`/`MonthPayload` need no
/// annotation of their own: they're constructed inside this function, so
/// rustc's "never constructed" check doesn't fire on them once this one item
/// is allowed.
#[allow(dead_code)]
pub fn assemble_month(events: &[StoredEvent], year: i32, month: u32, tz: &str) -> MonthPayload {
    use jiff::civil::date;

    let grid_start_ms = month_grid_start_ms(year, month, tz);
    let bounds = n_day_boundaries(grid_start_ms, 42, tz);

    let month_start_ms = local_midnight_ms(date(year as i16, month as i8, 1), tz);
    let (next_year, next_month) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    let next_month_start_ms = local_midnight_ms(date(next_year as i16, next_month as i8, 1), tz);

    let suppressed = suppressed_slots(events);

    let rows = (0..6)
        .map(|r| {
            let row_bounds = &bounds[r * 7..=r * 7 + 7];
            let row_start = row_bounds[0];
            let row_end = row_bounds[7];

            let mut day_events: Vec<Vec<UiEvent>> = vec![Vec::new(); 7];
            let mut bar_events: Vec<UiEvent> = Vec::new();
            let mut segments: Vec<Segment> = Vec::new();

            for src in events {
                // A cancelled exception exists only to record that an
                // occurrence was deleted; see `assemble_days`.
                if src.status == "cancelled" {
                    continue;
                }
                for iv in occurrences(src, row_start, row_end) {
                    if suppressed.contains(&(src.calendar_id, src.google_id.as_str(), iv.start_ms)) {
                        continue;
                    }
                    if src.is_all_day {
                        let start_col = signed_column(row_bounds, iv.start_ms);
                        // Google's all-day end is exclusive, so the last
                        // covered day is one millisecond before it.
                        let end_col = signed_column(row_bounds, iv.end_ms - 1);
                        segments.push(Segment { idx: bar_events.len(), start_col, end_col });
                        bar_events.push(to_ui(src, iv.start_ms, iv.end_ms));
                    } else if let Some(col) = timed_column(row_bounds, &iv) {
                        day_events[col].push(to_ui(src, iv.start_ms, iv.end_ms));
                    }
                }
            }

            // Three lanes, matching the spec's month rows.
            let (bars, bar_overflow) = pack_lanes(&segments, 7, 3);

            let cells = (0..7)
                .map(|c| {
                    let mut timed = std::mem::take(&mut day_events[c]);
                    timed.sort_by_key(|e| e.start_ms);
                    let start_ms = row_bounds[c];
                    MonthCell {
                        start_ms,
                        end_ms: row_bounds[c + 1],
                        in_month: start_ms >= month_start_ms && start_ms < next_month_start_ms,
                        timed,
                    }
                })
                .collect();

            MonthRow { cells, bars, bar_events, bar_overflow }
        })
        .collect();

    MonthPayload { rows, year, month }
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

    #[test]
    fn one_day_and_seven_days_agree_about_the_day_they_share() {
        // This is the guard that Day is genuinely the week engine at n=1 rather
        // than a parallel implementation that will drift. If someone later
        // "optimises" assemble_days for the n=1 case, this fails.
        let evs = vec![ev("standup", MON + 9 * 3_600_000, MON + 9 * 3_600_000 + 30 * 60_000, false)];

        let week = assemble_days(&evs, MON, 7, "Europe/Sofia");
        let day = assemble_days(&evs, week.days[0].start_ms, 1, "Europe/Sofia");

        assert_eq!(day.days.len(), 1);
        assert_eq!(day.days[0].start_ms, week.days[0].start_ms);
        assert_eq!(day.days[0].end_ms, week.days[0].end_ms);
        assert_eq!(
            day.days[0].events.len(),
            week.days[0].events.len(),
            "the same day assembled alone and as part of a week disagreed"
        );
    }

    #[test]
    fn a_single_day_window_still_bounds_its_all_day_lane() {
        // pack_lanes is called with `n` as row_len; passing 7 for a one-day view
        // would let an all-day event claim columns that do not exist.
        let evs = vec![ev("trip", MON, MON + 3 * DAY, true)];
        let day = assemble_days(&evs, MON, 1, "Europe/Sofia");
        for lane in &day.all_day {
            assert!(lane.end_col < 1, "a 1-day view produced column {}", lane.end_col);
        }
    }

    fn all_day_event(start: i64, end: i64) -> omacal_store::StoredEvent {
        ev(&format!("allday_{start}"), start, end, true)
    }

    fn timed_event(start: i64, end: i64) -> omacal_store::StoredEvent {
        ev(&format!("timed_{start}"), start, end, false)
    }

    #[test]
    fn august_2026_starts_on_a_saturday_and_needs_six_rows() {
        // August 2026 begins Sat 1 Aug, so the grid runs Mon 27 Jul - Sun 6 Sep.
        // It exercises leading out-of-month days, trailing ones, and six rows.
        let m = assemble_month(&[], 2026, 8, "Europe/Sofia");
        assert_eq!(m.rows.len(), 6);
        assert_eq!(m.rows[0].cells.len(), 7);
        assert_eq!(m.rows[0].cells[0].start_ms, 1785099600000, "grid must start Mon 27 Jul");
        assert!(!m.rows[0].cells[0].in_month, "27 Jul belongs to July");
        assert!(m.rows[0].cells[5].in_month, "1 Aug is a Saturday, column 5");
        assert!(!m.rows[5].cells[6].in_month, "the last cell belongs to September");
    }

    #[test]
    fn a_month_that_fits_in_five_rows_still_renders_six() {
        // Otherwise the grid changes height as you page through the year.
        let m = assemble_month(&[], 2026, 2, "Europe/Sofia");
        assert_eq!(m.rows.len(), 6);
    }

    /// June 2026's 1st is itself a Monday — the one shape every other weekday
    /// absorbs. A backward search that steps one day too far before checking
    /// converges on the same Monday for every other starting weekday, and
    /// only misfires here, landing a week early. Value computed independently
    /// (Europe/Sofia local midnight, cross-checked against the plan's
    /// `AUG_GRID_START` derivation method): 2026-06-01 00:00 Sofia =
    /// 1780261200000.
    #[test]
    fn a_month_that_starts_on_a_monday_has_no_leading_days() {
        let m = assemble_month(&[], 2026, 6, "Europe/Sofia");
        assert_eq!(m.rows[0].cells[0].start_ms, 1780261200000, "grid must start Mon 1 Jun");
        assert!(m.rows[0].cells[0].in_month, "1 Jun is itself the Monday the grid starts on");
    }

    /// January 2026 opens on a Thursday, so the Monday on or before it falls
    /// in the *previous* calendar year: 2025-12-29, a boundary the naive "same
    /// year" assumption would get wrong. Value computed independently
    /// (Europe/Sofia local midnight): 2025-12-29 00:00 Sofia = 1766959200000.
    #[test]
    fn a_grid_start_can_fall_in_the_previous_year() {
        let m = assemble_month(&[], 2026, 1, "Europe/Sofia");
        assert_eq!(m.rows[0].cells[0].start_ms, 1766959200000, "grid must start Mon 29 Dec 2025");
        assert!(!m.rows[0].cells[0].in_month, "29 Dec 2025 belongs to December, not January");
    }

    #[test]
    fn a_multi_day_event_crossing_a_row_boundary_appears_in_both_rows_clipped() {
        // Sun 2 Aug -> Tue 4 Aug straddles the first row's end. It must appear
        // in both rows, clipped to each, and never as one event counted
        // twice.
        //
        // The brief's epoch values (1785963600000, 1786222800000) land on
        // Thu 6 Aug and Sun 9 Aug in Europe/Sofia, not Sun 2 / Tue 4 as
        // named — verified against the plan's fixture table
        // (`docs/superpowers/plans/2026-08-06-omacal-day-month-views.md`,
        // where `AUG_1` = Sat 2026-08-01 00:00 Sofia = 1785531600000).
        // Recomputed here instead of adjusting the assertions to match
        // whatever the code produces: start = Sun 2 Aug 00:00 Sofia
        // (1785618000000), end = Wed 5 Aug 00:00 Sofia (1785877200000,
        // exclusive per Google's all-day convention), covering Sun 2 - Tue
        // 4 Aug inclusive.
        let evs = vec![all_day_event(1785618000000, 1785877200000)];
        let m = assemble_month(&evs, 2026, 8, "Europe/Sofia");
        // `bars: Vec<Lane>` — each `Lane` is already one placed, row-clipped
        // segment (see `omacal_core::lanes::Lane`), not a container of
        // segments, so the count of bars *is* the count of segments placed
        // in the row.
        assert_eq!(m.rows[0].bars.len(), 1, "row 0 should carry the Sunday tail");
        assert_eq!(m.rows[1].bars.len(), 1, "row 1 should carry the Mon-Tue head");
        for lane in &m.rows[0].bars {
            assert!(lane.end_col <= 6, "a segment escaped its row: {}", lane.end_col);
        }
    }

    #[test]
    fn timed_events_land_in_their_own_day_sorted() {
        let evs = vec![
            timed_event(1786341600000 + 3 * 3_600_000, 1786341600000 + 4 * 3_600_000),
            timed_event(1786341600000, 1786341600000 + 30 * 60_000),
        ];
        let m = assemble_month(&evs, 2026, 8, "Europe/Sofia");
        // Mon 10 Aug is row 2, column 0.
        let cell = &m.rows[2].cells[0];
        assert_eq!(cell.timed.len(), 2);
        assert!(cell.timed[0].start_ms < cell.timed[1].start_ms, "not sorted by start");
    }
}
