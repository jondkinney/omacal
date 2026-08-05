use serde::Serialize;

/// An event clipped to one row, in **inclusive** column indices. Values may
/// fall outside the row; `pack_lanes` clips them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    pub idx: usize,
    pub start_col: i32,
    pub end_col: i32,
}

/// A placed segment.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Lane {
    pub idx: usize,
    pub lane: u8,
    /// Inclusive, clipped to the row.
    pub start_col: u16,
    /// Inclusive, clipped to the row.
    pub end_col: u16,
    /// True when the event began before this row (render a flat left edge).
    pub cont_left: bool,
    /// True when the event continues past this row (render a flat right edge).
    pub cont_right: bool,
}

/// Greedily packs segments into horizontal lanes, longest first, so that the
/// most significant spans sit closest to the day numbers.
///
/// Returns `(placed, overflowed)`, where `overflowed` holds the input indices
/// that did not fit within `max_lanes` — the caller renders them as "+N more".
pub fn pack_lanes(segs: &[Segment], row_len: u16, max_lanes: u8) -> (Vec<Lane>, Vec<usize>) {
    if row_len == 0 || max_lanes == 0 {
        return (Vec::new(), Vec::new());
    }
    let last = (row_len - 1) as i32;

    // Clip to the row, dropping anything fully outside it.
    let mut clipped: Vec<Lane> = segs
        .iter()
        .filter(|s| s.end_col >= 0 && s.start_col <= last && s.start_col <= s.end_col)
        .map(|s| Lane {
            idx: s.idx,
            lane: 0,
            start_col: s.start_col.max(0) as u16,
            end_col: s.end_col.min(last) as u16,
            cont_left: s.start_col < 0,
            cont_right: s.end_col > last,
        })
        .collect();

    // Longest first, then leftmost, then by index for determinism.
    clipped.sort_by_key(|l| {
        let width = l.end_col as i32 - l.start_col as i32;
        (-width, l.start_col, l.idx)
    });

    let mut lanes: Vec<Vec<(u16, u16)>> = Vec::new();
    let mut placed: Vec<Lane> = Vec::new();
    let mut overflow: Vec<usize> = Vec::new();

    'next: for mut item in clipped {
        for (n, occupied) in lanes.iter_mut().enumerate() {
            let free = occupied
                .iter()
                .all(|&(a, b)| item.end_col < a || b < item.start_col);
            if free {
                occupied.push((item.start_col, item.end_col));
                item.lane = n as u8;
                placed.push(item);
                continue 'next;
            }
        }
        if lanes.len() < max_lanes as usize {
            lanes.push(vec![(item.start_col, item.end_col)]);
            item.lane = (lanes.len() - 1) as u8;
            placed.push(item);
        } else {
            overflow.push(item.idx);
        }
    }

    placed.sort_by_key(|l| (l.lane, l.start_col));
    overflow.sort_unstable();
    (placed, overflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(idx: usize, a: i32, b: i32) -> Segment {
        Segment { idx, start_col: a, end_col: b }
    }

    #[test]
    fn a_single_segment_takes_lane_zero() {
        let (lanes, over) = pack_lanes(&[seg(0, 2, 5)], 28, 3);
        assert_eq!(lanes[0].lane, 0);
        assert_eq!(lanes[0].start_col, 2);
        assert_eq!(lanes[0].end_col, 5);
        assert!(over.is_empty());
    }

    #[test]
    fn non_overlapping_segments_share_lane_zero() {
        let (lanes, _) = pack_lanes(&[seg(0, 0, 3), seg(1, 5, 8)], 28, 3);
        assert_eq!(lanes[0].lane, 0);
        assert_eq!(lanes[1].lane, 0);
    }

    #[test]
    fn adjacent_segments_share_a_lane() {
        // Inclusive columns: 0..=3 and 4..=6 touch but do not overlap.
        let (lanes, _) = pack_lanes(&[seg(0, 0, 3), seg(1, 4, 6)], 28, 3);
        assert_eq!(lanes[0].lane, 0);
        assert_eq!(lanes[1].lane, 0);
    }

    #[test]
    fn overlapping_segments_stack_into_lanes() {
        let (lanes, _) = pack_lanes(&[seg(0, 0, 5), seg(1, 3, 8)], 28, 3);
        assert_eq!(lanes[0].lane, 0);
        assert_eq!(lanes[1].lane, 1);
    }

    #[test]
    fn longest_segment_wins_lane_zero() {
        // Declared shortest-first, but the long one should sit on top.
        let (lanes, _) = pack_lanes(&[seg(0, 3, 4), seg(1, 0, 20)], 28, 3);
        let long = lanes.iter().find(|l| l.idx == 1).unwrap();
        assert_eq!(long.lane, 0);
    }

    #[test]
    fn segments_are_clipped_to_the_row_and_flagged() {
        let (lanes, _) = pack_lanes(&[seg(0, -5, 40)], 28, 3);
        assert_eq!(lanes[0].start_col, 0);
        assert_eq!(lanes[0].end_col, 27);
        assert!(lanes[0].cont_left);
        assert!(lanes[0].cont_right);
    }

    #[test]
    fn a_segment_inside_the_row_is_not_flagged() {
        let (lanes, _) = pack_lanes(&[seg(0, 1, 26)], 28, 3);
        assert!(!lanes[0].cont_left);
        assert!(!lanes[0].cont_right);
    }

    #[test]
    fn segments_entirely_outside_the_row_are_dropped() {
        let (lanes, over) = pack_lanes(&[seg(0, 40, 50), seg(1, -9, -2)], 28, 3);
        assert!(lanes.is_empty());
        assert!(over.is_empty());
    }

    #[test]
    fn overflow_beyond_max_lanes_is_reported_not_placed() {
        let segs: Vec<Segment> = (0..5).map(|i| seg(i, 0, 10)).collect();
        let (lanes, over) = pack_lanes(&segs, 28, 3);
        assert_eq!(lanes.len(), 3);
        assert_eq!(over.len(), 2);
    }

    /// The invariant: two segments may never share a lane while overlapping.
    #[test]
    fn no_two_segments_share_a_lane_while_overlapping() {
        let segs: Vec<Segment> = (0..30)
            .map(|i| {
                let a = (i as i32 * 3) % 25;
                seg(i, a, a + (i as i32 % 6))
            })
            .collect();
        let (lanes, _) = pack_lanes(&segs, 28, 8);
        for a in &lanes {
            for b in &lanes {
                if a.idx == b.idx || a.lane != b.lane {
                    continue;
                }
                assert!(
                    a.end_col < b.start_col || b.end_col < a.start_col,
                    "segments {} and {} overlap but share lane {}", a.idx, b.idx, a.lane
                );
            }
        }
    }
}
