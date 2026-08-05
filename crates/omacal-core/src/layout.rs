use serde::Serialize;

/// A half-open time interval in epoch milliseconds: `[start_ms, end_ms)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Interval {
    pub start_ms: i64,
    pub end_ms: i64,
}

impl Interval {
    #[allow(dead_code)]
    fn overlaps(&self, other: &Interval) -> bool {
        self.start_ms < other.end_ms && other.start_ms < self.end_ms
    }
}

/// Computed geometry for one event in a day column.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Placed {
    /// Index into the input slice.
    pub idx: usize,
    /// 0-based column within this event's cluster.
    pub column: u8,
    /// Total columns in this event's cluster.
    pub columns: u8,
    /// Fraction of the day window, 0.0..=1.0.
    pub top: f32,
    /// Fraction of the day window; always > 0.
    pub height: f32,
}

/// Minimum rendered height, as a fraction of the window, so that a zero-length
/// or very short event is still clickable.
const MIN_HEIGHT: f32 = 0.004;

/// Assigns overlapping events to side-by-side columns and computes vertical
/// geometry as fractions of `[day_start_ms, day_end_ms)`.
///
/// Events are grouped into *clusters*: maximal runs of events connected by
/// overlap. Every event in a cluster is rendered at the cluster's full column
/// count so that edges line up, which is what makes a collision read as one.
pub fn lay_out_day(events: &[Interval], day_start_ms: i64, day_end_ms: i64) -> Vec<Placed> {
    if events.is_empty() {
        return Vec::new();
    }

    // Sort indices by start, then by longest-first so big blocks take column 0.
    let mut order: Vec<usize> = (0..events.len()).collect();
    order.sort_by_key(|&i| {
        let e = events[i];
        (e.start_ms, -(e.end_ms - e.start_ms), i as i64)
    });

    let mut column_of = vec![0u8; events.len()];
    let mut columns_of = vec![1u8; events.len()];

    // Events currently occupying each column, as (end_ms, idx).
    let mut active: Vec<Option<(i64, usize)>> = Vec::new();
    let mut cluster: Vec<usize> = Vec::new();
    let mut cluster_end: i64 = i64::MIN;

    for &i in &order {
        let ev = events[i];

        // A cluster ends when an event starts at or after every active event's end.
        if !cluster.is_empty() && ev.start_ms >= cluster_end {
            let width = active.len() as u8;
            for &c in &cluster {
                columns_of[c] = width;
            }
            cluster.clear();
            active.clear();
            cluster_end = i64::MIN;
        }

        // First column whose occupant has already finished.
        let slot = active
            .iter()
            .position(|s| s.is_none_or(|(end, _)| end <= ev.start_ms));

        let col = match slot {
            Some(c) => {
                active[c] = Some((ev.end_ms, i));
                c
            }
            None => {
                active.push(Some((ev.end_ms, i)));
                active.len() - 1
            }
        };

        column_of[i] = col as u8;
        cluster.push(i);
        cluster_end = cluster_end.max(ev.end_ms);
    }

    if !cluster.is_empty() {
        let width = active.len() as u8;
        for &c in &cluster {
            columns_of[c] = width;
        }
    }

    let span = (day_end_ms - day_start_ms).max(1) as f32;
    events
        .iter()
        .enumerate()
        .map(|(idx, ev)| {
            let start = ev.start_ms.clamp(day_start_ms, day_end_ms);
            let end = ev.end_ms.clamp(start, day_end_ms);
            let top = (start - day_start_ms) as f32 / span;
            let height = ((end - start) as f32 / span).max(MIN_HEIGHT);
            Placed { idx, column: column_of[idx], columns: columns_of[idx], top, height }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 0;
    const END: i64 = 24 * 3_600_000;

    fn iv(h_start: f64, h_end: f64) -> Interval {
        Interval { start_ms: (h_start * 3_600_000.0) as i64, end_ms: (h_end * 3_600_000.0) as i64 }
    }

    #[test]
    fn disjoint_events_each_take_the_full_width() {
        let out = lay_out_day(&[iv(9.0, 10.0), iv(11.0, 12.0)], DAY, END);
        assert_eq!(out[0].columns, 1);
        assert_eq!(out[1].columns, 1);
        assert_eq!(out[0].column, 0);
        assert_eq!(out[1].column, 0);
    }

    #[test]
    fn identical_times_split_evenly() {
        let out = lay_out_day(&[iv(10.0, 11.0), iv(10.0, 11.0)], DAY, END);
        assert_eq!(out[0].columns, 2);
        assert_eq!(out[1].columns, 2);
        assert_eq!(out[0].column, 0);
        assert_eq!(out[1].column, 1);
    }

    #[test]
    fn partial_overlap_splits_the_cluster() {
        // 10:00-11:00 and 10:30-11:30 collide only in the middle half hour.
        let out = lay_out_day(&[iv(10.0, 11.0), iv(10.5, 11.5)], DAY, END);
        assert_eq!(out[0].columns, 2);
        assert_eq!(out[1].column, 1);
    }

    #[test]
    fn three_way_pile_uses_three_columns() {
        let out = lay_out_day(&[iv(10.0, 12.0), iv(10.5, 11.0), iv(10.75, 11.5)], DAY, END);
        assert_eq!(out.iter().map(|p| p.columns).max().unwrap(), 3);
        assert_eq!(out[0].column, 0);
        assert_eq!(out[1].column, 1);
        assert_eq!(out[2].column, 2);
    }

    #[test]
    fn a_gap_starts_a_new_cluster() {
        // First two collide; the third is alone and must not be widened to 2 columns.
        let out = lay_out_day(&[iv(9.0, 10.0), iv(9.5, 10.0), iv(14.0, 15.0)], DAY, END);
        assert_eq!(out[0].columns, 2);
        assert_eq!(out[2].columns, 1);
    }

    #[test]
    fn touching_events_do_not_overlap() {
        // 09:00-10:00 and 10:00-11:00 share only an instant; that is not a collision.
        let out = lay_out_day(&[iv(9.0, 10.0), iv(10.0, 11.0)], DAY, END);
        assert_eq!(out[0].columns, 1);
        assert_eq!(out[1].columns, 1);
    }

    #[test]
    fn geometry_is_a_fraction_of_the_window() {
        let out = lay_out_day(&[iv(6.0, 12.0)], DAY, END);
        assert!((out[0].top - 0.25).abs() < 1e-6);
        assert!((out[0].height - 0.25).abs() < 1e-6);
    }

    #[test]
    fn events_are_clamped_to_the_window() {
        let out = lay_out_day(&[iv(-2.0, 2.0)], DAY, END);
        assert!((out[0].top - 0.0).abs() < 1e-6);
        assert!(out[0].height > 0.0);
    }

    #[test]
    fn zero_length_events_still_get_a_visible_height() {
        let out = lay_out_day(&[iv(9.0, 9.0)], DAY, END);
        assert!(out[0].height > 0.0);
    }

    /// The invariant that matters: two events may never share a column while
    /// overlapping in time.
    #[test]
    fn no_two_events_share_a_column_while_overlapping() {
        let evs: Vec<Interval> = (0..40)
            .map(|i| {
                let s = (i as i64 * 7 % 20) * 1_800_000;
                Interval { start_ms: s, end_ms: s + ((i as i64 % 5) + 1) * 1_800_000 }
            })
            .collect();
        let out = lay_out_day(&evs, DAY, END);
        for a in &out {
            for b in &out {
                if a.idx == b.idx || a.column != b.column {
                    continue;
                }
                let (x, y) = (&evs[a.idx], &evs[b.idx]);
                assert!(
                    x.start_ms >= y.end_ms || y.start_ms >= x.end_ms,
                    "events {} and {} overlap but share column {}", a.idx, b.idx, a.column
                );
            }
        }
    }
}
