// The window on a padded week payload, and the arithmetic of sliding it.
//
// Since 2026-09-03 the day grids fetch more days than they show: `padFor`
// either side of the window, in one payload, so a sideways swipe has real
// columns to reveal before the fetch for the new window lands (the old
// pan jumped a day per 90px of wheel and refetched on every jump — content
// arrived in lurches, and nothing moved under the finger). Everything that
// is about *what is on screen* reads the window; only the grid's track ever
// draws the padding. Pure, so `weekwindow.spec.ts` pins each rule.

import type { Lane, WeekPayload } from './api';

/** Days of padding either side of a window of `visible` days: the window's
 *  own width, so a swipe can travel a whole window before it outruns the
 *  payload, and never fewer than three, so Day view has somewhere to go. */
export const padFor = (visible: number) => Math.max(3, visible);

/**
 * Where the window starts in `days`: the index of the day beginning at
 * `visibleStartMs`, or -1 for a payload that does not hold it — one still
 * on screen from before the window jumped (a view switch, Today from far
 * away), or an unpadded one from a stub. The callers then show the whole
 * payload, which is what showed before it was padded: the honest answer to
 * "the days you asked for are not here yet" is the days that are, not a
 * window's worth of whatever happens to sit at the start.
 */
export function visibleIndex(days: { start_ms: number }[], visibleStartMs: number): number {
  return days.findIndex((d) => d.start_ms === visibleStartMs);
}

/**
 * The payload as if only `count` days from `from` had been fetched.
 *
 * Days are sliced; an all-day lane is kept if any of it falls inside, cut to
 * the edges, and marked continuing where it was cut — the same `cont_left`/
 * `cont_right` the backend sets for a span crossing the fetched range, so
 * the band draws a padded payload's window exactly as it drew the unpadded
 * fetch. `all_day_events` is left whole: lanes index into it. `overflow` is
 * left whole too — an event the wider packing pushed off the band is a
 * judgement about the wider range, and re-packing here would be a second
 * lane packer to disagree with the first.
 */
export function sliceWeek(week: WeekPayload, from: number, count: number): WeekPayload {
  if (from === 0 && count >= week.days.length) return week;
  const last = from + count - 1;
  const all_day: Lane[] = [];
  for (const lane of week.all_day) {
    if (lane.end_col < from || lane.start_col > last) continue;
    all_day.push({
      ...lane,
      start_col: Math.max(lane.start_col, from) - from,
      end_col: Math.min(lane.end_col, last) - from,
      cont_left: lane.cont_left || lane.start_col < from,
      cont_right: lane.cont_right || lane.end_col > last,
    });
  }
  return { ...week, days: week.days.slice(from, from + count), all_day };
}

/**
 * The whole days a pan has crossed, and what is left.
 *
 * `panDays` is the finger's travel in columns, positive when the content has
 * moved right (towards earlier days). Whole columns are handed to the app as
 * a shift of the window — the opposite sign, since content moving right
 * means the window moving left — and the fraction stays on the track.
 */
export function panCommit(panDays: number): { shift: number; rest: number } {
  const whole = Math.trunc(panDays);
  // `+ 0` turns `-0` into `0` — `drag.ts`'s `colsMoved` has the story.
  return { shift: -whole + 0, rest: panDays - whole };
}

/**
 * Where a pan settles when the fingers lift: the nearest whole column. More
 * than half a column over commits one more day (`shift`), and the track
 * animates from what is then left (`from`) back to zero.
 */
export function snapPlan(panDays: number): { shift: number; from: number } {
  const nearest = Math.round(panDays);
  return { shift: -nearest + 0, from: panDays - nearest };
}

/**
 * Whether `days` holds the window *and* `margin` days beyond it on each
 * side — the case in which a fetch can wait. A pan inside the padding needs
 * nothing fetched to draw, so the refetch that recentres the padding is
 * deferred until the gesture settles (App); fetching per day crossed
 * re-rendered three weeks of blocks under every wheel event, and that was
 * the lag (2026-09-03). At the padding's edge the fetch is immediate again:
 * one more column and there would be nothing to slide into.
 */
export function windowHeld(
  days: { start_ms: number }[], visibleStartMs: number, visible: number, margin: number,
): boolean {
  const i = visibleIndex(days, visibleStartMs);
  return i >= margin && i + visible + margin <= days.length;
}

/** A wheel event's contribution to the pan, for the velocity estimate. */
export type PanSample = { t: number; days: number };

/**
 * The pan's speed in columns per millisecond over the samples' span, or 0
 * for fewer than two of them. Only the last `WINDOW_MS` of samples count,
 * so a long slow drag that ends in a flick reports the flick.
 */
export function velocityOf(samples: PanSample[], windowMs = 100): number {
  if (samples.length < 2) return 0;
  const last = samples[samples.length - 1].t;
  const recent = samples.filter((s) => last - s.t <= windowMs);
  if (recent.length < 2) return 0;
  const span = recent[recent.length - 1].t - recent[0].t;
  if (span <= 0) return 0;
  // The first sample marks the start of the span; its own days were
  // travelled before it.
  const days = recent.slice(1).reduce((acc, s) => acc + s.days, 0);
  return days / span;
}

/** Momentum's time constant: after the fingers lift, the speed decays as
 *  e^(-t/τ), so a flick at `v` columns/ms travels `v * τ` more columns. 400
 *  puts a brisk flick two to three days on, a hard one a week. Linux has no
 *  inertia of its own on a wheel — libinput stops at lift — and macOS's own
 *  momentum events keep our lull from firing until they have decayed, by
 *  which time the speed is low and this adds nothing. */
export const FLING_TAU_MS = 400;
/** Below this, the fingers stopped rather than flicked: settle at once. */
export const FLING_MIN_V = 0.0015;
/** Never further than the padding on one side minus a column: momentum
 *  that outran the payload would slide into nothing. */
export const FLING_MAX_DAYS = 6;

/** How far a fling at `v` columns/ms will travel, capped. Signed. */
export function flingTravel(v: number, tau = FLING_TAU_MS, cap = FLING_MAX_DAYS): number {
  if (Math.abs(v) < FLING_MIN_V) return 0;
  const d = v * tau;
  return Math.max(-cap, Math.min(cap, d));
}

/** Where a fling stands `t` ms after lift, as a fraction of its travel:
 *  1 - e^(-t/τ), which is the integral of the decaying speed. */
export const flingProgress = (t: number, tau = FLING_TAU_MS) => 1 - Math.exp(-t / tau);
