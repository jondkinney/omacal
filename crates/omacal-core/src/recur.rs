use crate::layout::Interval;
use chrono::TimeZone;
use rrule::{RRuleSet, Tz};
use std::str::FromStr;

#[derive(Debug, thiserror::Error)]
pub enum RecurError {
    #[error("unknown time zone: {0}")]
    UnknownTimeZone(String),
    #[error("invalid recurrence rule: {0}")]
    InvalidRule(String),
    #[error("timestamp out of range: {0}")]
    OutOfRange(i64),
}

/// A recurring (or single) event as stored: a start instant, the IANA zone it
/// was authored in, a duration, and Google's raw recurrence lines.
#[derive(Debug, Clone)]
pub struct Series<'a> {
    pub dtstart_ms: i64,
    pub dtstart_tz: &'a str,
    pub duration_ms: i64,
    pub is_all_day: bool,
    pub recurrence: &'a [String],
}

fn to_chrono(ms: i64) -> Result<chrono::DateTime<Tz>, RecurError> {
    Tz::UTC
        .timestamp_millis_opt(ms)
        .single()
        .ok_or(RecurError::OutOfRange(ms))
}

/// Renders the DTSTART line in the series' own zone, which is what makes a
/// "09:00 every Monday" meeting stay at 09:00 across a DST transition.
fn dtstart_line(series: &Series) -> Result<String, RecurError> {
    let zone: chrono_tz::Tz = series
        .dtstart_tz
        .parse()
        .map_err(|_| RecurError::UnknownTimeZone(series.dtstart_tz.to_string()))?;
    let local = chrono::Utc
        .timestamp_millis_opt(series.dtstart_ms)
        .single()
        .ok_or(RecurError::OutOfRange(series.dtstart_ms))?
        .with_timezone(&zone);

    Ok(if series.is_all_day {
        format!("DTSTART;VALUE=DATE:{}", local.format("%Y%m%d"))
    } else {
        format!(
            "DTSTART;TZID={}:{}",
            series.dtstart_tz,
            local.format("%Y%m%dT%H%M%S")
        )
    })
}

/// Expands `series` into concrete intervals overlapping `[from_ms, to_ms)`.
///
/// `limit` bounds the number of occurrences generated, guarding against
/// unbounded rules such as `FREQ=MINUTELY` with no `COUNT`/`UNTIL`.
pub fn expand(
    series: &Series,
    from_ms: i64,
    to_ms: i64,
    limit: u16,
) -> Result<Vec<Interval>, RecurError> {
    // Validate the zone even when there is no rule, so callers get a
    // consistent error rather than a silent pass.
    let dtstart = dtstart_line(series)?;

    if series.recurrence.is_empty() {
        let end = series.dtstart_ms + series.duration_ms;
        return Ok(if series.dtstart_ms < to_ms && end > from_ms {
            vec![Interval { start_ms: series.dtstart_ms, end_ms: end }]
        } else {
            Vec::new()
        });
    }

    let mut source = String::with_capacity(128);
    source.push_str(&dtstart);
    for line in series.recurrence {
        source.push('\n');
        source.push_str(line.trim());
    }

    let set = RRuleSet::from_str(&source)
        .map_err(|e| RecurError::InvalidRule(format!("{e}: {source}")))?;

    // Widen the query by the event duration so an occurrence that started
    // before the window but is still running is not dropped.
    let query_from = from_ms.saturating_sub(series.duration_ms);
    let result = set
        .after(to_chrono(query_from)?)
        .before(to_chrono(to_ms)?)
        .all(limit);

    Ok(result
        .dates
        .into_iter()
        .map(|d| {
            let start = d.timestamp_millis();
            Interval { start_ms: start, end_ms: start + series.duration_ms }
        })
        .filter(|i| i.start_ms < to_ms && i.end_ms > from_ms)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Monday 2026-08-03 09:00:00 Europe/Sofia == 06:00:00Z (EEST, UTC+3).
    /// Verify with: `python3 -c "import datetime as d; print(int(d.datetime(2026,8,3,6,tzinfo=d.timezone.utc).timestamp()*1000))"`
    const MON_0900_SOFIA: i64 = 1_785_736_800_000;
    const HOUR: i64 = 3_600_000;
    const DAY: i64 = 24 * HOUR;

    fn weekly(rules: &[&str]) -> Vec<String> {
        rules.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_non_recurring_event_yields_itself_when_in_window() {
        let s = Series {
            dtstart_ms: MON_0900_SOFIA, dtstart_tz: "Europe/Sofia",
            duration_ms: 30 * 60_000, is_all_day: false, recurrence: &[],
        };
        let out = expand(&s, MON_0900_SOFIA - DAY, MON_0900_SOFIA + DAY, 50).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].start_ms, MON_0900_SOFIA);
        assert_eq!(out[0].end_ms, MON_0900_SOFIA + 30 * 60_000);
    }

    #[test]
    fn a_non_recurring_event_outside_the_window_yields_nothing() {
        let s = Series {
            dtstart_ms: MON_0900_SOFIA, dtstart_tz: "Europe/Sofia",
            duration_ms: 30 * 60_000, is_all_day: false, recurrence: &[],
        };
        let out = expand(&s, MON_0900_SOFIA + 10 * DAY, MON_0900_SOFIA + 20 * DAY, 50).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn a_daily_standup_yields_one_per_day() {
        let s = Series {
            dtstart_ms: MON_0900_SOFIA, dtstart_tz: "Europe/Sofia",
            duration_ms: 30 * 60_000, is_all_day: false,
            recurrence: &weekly(&["RRULE:FREQ=DAILY"]),
        };
        // Window covering Mon..Fri inclusive.
        let out = expand(&s, MON_0900_SOFIA - HOUR, MON_0900_SOFIA + 5 * DAY, 50).unwrap();
        assert_eq!(out.len(), 5);
        assert_eq!(out[0].start_ms, MON_0900_SOFIA);
        assert_eq!(out[1].start_ms, MON_0900_SOFIA + DAY);
    }

    #[test]
    fn every_instance_keeps_the_series_duration() {
        let s = Series {
            dtstart_ms: MON_0900_SOFIA, dtstart_tz: "Europe/Sofia",
            duration_ms: 90 * 60_000, is_all_day: false,
            recurrence: &weekly(&["RRULE:FREQ=DAILY;COUNT=3"]),
        };
        let out = expand(&s, MON_0900_SOFIA - HOUR, MON_0900_SOFIA + 5 * DAY, 50).unwrap();
        for i in &out {
            assert_eq!(i.end_ms - i.start_ms, 90 * 60_000);
        }
    }

    #[test]
    fn exdate_removes_an_instance() {
        let s = Series {
            dtstart_ms: MON_0900_SOFIA, dtstart_tz: "Europe/Sofia",
            duration_ms: 30 * 60_000, is_all_day: false,
            recurrence: &weekly(&[
                "RRULE:FREQ=DAILY",
                // Tuesday 2026-08-04 09:00 Sofia == 06:00Z
                "EXDATE;TZID=Europe/Sofia:20260804T090000",
            ]),
        };
        let out = expand(&s, MON_0900_SOFIA - HOUR, MON_0900_SOFIA + 3 * DAY, 50).unwrap();
        assert!(out.iter().all(|i| i.start_ms != MON_0900_SOFIA + DAY));
    }

    #[test]
    fn count_is_respected() {
        let s = Series {
            dtstart_ms: MON_0900_SOFIA, dtstart_tz: "Europe/Sofia",
            duration_ms: 30 * 60_000, is_all_day: false,
            recurrence: &weekly(&["RRULE:FREQ=DAILY;COUNT=2"]),
        };
        let out = expand(&s, MON_0900_SOFIA - HOUR, MON_0900_SOFIA + 30 * DAY, 50).unwrap();
        assert_eq!(out.len(), 2);
    }

    /// The reason `jiff`/IANA zones matter. Europe/Sofia leaves DST on
    /// 2026-10-25. A 09:00 local weekly meeting must stay at 09:00 local,
    /// which means its UTC instant shifts by an hour across the boundary.
    #[test]
    fn a_local_time_series_survives_a_dst_transition() {
        // Monday 2026-09-28 09:00 Sofia (EEST, +3) == 06:00Z
        let sep28 = 1_790_575_200_000;
        let s = Series {
            dtstart_ms: sep28, dtstart_tz: "Europe/Sofia",
            duration_ms: HOUR, is_all_day: false,
            recurrence: &weekly(&["RRULE:FREQ=WEEKLY;BYDAY=MO"]),
        };
        let out = expand(&s, sep28 - HOUR, sep28 + 45 * DAY, 50).unwrap();
        let deltas: Vec<i64> = out.windows(2).map(|w| w[1].start_ms - w[0].start_ms).collect();
        // Exactly one gap is 7 days + 1 hour: the week DST ends.
        assert_eq!(deltas.iter().filter(|&&d| d == 7 * DAY + HOUR).count(), 1,
                   "expected one DST-adjusted gap, got {:?}", deltas);
        assert!(deltas.iter().all(|&d| d == 7 * DAY || d == 7 * DAY + HOUR));
    }

    #[test]
    fn the_limit_caps_runaway_expansion() {
        let s = Series {
            dtstart_ms: MON_0900_SOFIA, dtstart_tz: "Europe/Sofia",
            duration_ms: 30 * 60_000, is_all_day: false,
            recurrence: &weekly(&["RRULE:FREQ=MINUTELY"]),
        };
        let out = expand(&s, MON_0900_SOFIA, MON_0900_SOFIA + DAY, 10).unwrap();
        assert_eq!(out.len(), 10);
    }

    #[test]
    fn a_malformed_rule_is_an_error_not_a_panic() {
        let s = Series {
            dtstart_ms: MON_0900_SOFIA, dtstart_tz: "Europe/Sofia",
            duration_ms: 30 * 60_000, is_all_day: false,
            recurrence: &weekly(&["RRULE:FREQ=NONSENSE"]),
        };
        assert!(expand(&s, MON_0900_SOFIA, MON_0900_SOFIA + DAY, 10).is_err());
    }

    #[test]
    fn an_unknown_timezone_is_an_error_not_a_panic() {
        let s = Series {
            dtstart_ms: MON_0900_SOFIA, dtstart_tz: "Mars/Olympus_Mons",
            duration_ms: 30 * 60_000, is_all_day: false,
            recurrence: &weekly(&["RRULE:FREQ=DAILY"]),
        };
        assert!(expand(&s, MON_0900_SOFIA, MON_0900_SOFIA + DAY, 10).is_err());
    }
}
