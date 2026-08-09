/**
 * The drag geometry: given where a pointer went, what span results.
 *
 * **No DOM, no pointer events, no component.** Everything here is a function of
 * its arguments, which is what lets the off-by-ones live somewhere they can be
 * tested exhaustively — spec §7. The gesture (threshold, escape,
 * drop-where-it-started) is the browser's half and is not in this file.
 *
 * Two conventions are inherited from `WeekGrid`, not invented here, because the
 * arithmetic has to agree with what is on screen:
 *
 * - **The vertical axis is a fraction of the day's own span.** A day is 23 or
 *   25 hours across a transition, and `hourFrac`/`slotAt` already lay a column
 *   out by its real length. Dividing by a fixed 24 puts every drop after the
 *   transition an hour out.
 * - **Snapping is in local wall-clock minutes**, never by rounding the instant
 *   to a multiple of the interval. `slotAt` documents why: a zone offset at :45
 *   (Kathmandu, Chatham) has no half hour on a whole-half-hour UTC boundary, so
 *   epoch-rounding would offer 09:15 for a drop on the 09:30 line.
 */

/**
 * How far the pointer must travel, with the button held, before a press
 * becomes a drag rather than a click (spec §4).
 *
 * Four pixels. Without it every click on an event is a potential fifteen-minute
 * move, because no hand holds a mouse perfectly still between pressing and
 * releasing — and a user who can no longer open an event by clicking it has
 * lost more than dragging gains.
 */
export const DRAG_THRESHOLD_PX = 4;

/** The snap interval (spec §5): fifteen minutes, which is how meetings are
 *  actually scheduled and lands on clean times without fighting. */
export const SNAP_MS = 15 * 60_000;

/**
 * Whether a press that has travelled `dx`, `dy` has become a drag.
 *
 * Straight-line distance rather than either axis alone: a diagonal wander of
 * three pixels each way is five pixels of travel, and treating it as a click
 * because neither component reached four would make the threshold mean
 * something different in every direction.
 *
 * Here rather than in the component because it is arithmetic, and arithmetic
 * in a Svelte file is arithmetic without a table.
 */
export function beganDrag(dx: number, dy: number, threshold: number = DRAG_THRESHOLD_PX): boolean {
  return Math.hypot(dx, dy) >= threshold;
}

/** A span of time. The shape a drag produces and a write consumes. */
export type Span = { startMs: number; endMs: number };

/**
 * `ms` moved to the nearest `intervalMs` boundary, **counted in local
 * wall-clock minutes past the hour**.
 *
 * Ties round **up** — 7:30 into a 15-minute slot becomes 15, not 0. That is
 * `Math.round`'s own rule and the one that is easiest to say out loud:
 * *nearest, and ties go forward*. It is a decision rather than a detail,
 * because a drag sits on the midpoint every time the pointer is halfway
 * between two lines.
 *
 * `intervalMs` must divide an hour. 15 minutes does, which is spec §5's answer
 * and the only value this app passes; the constraint is what lets the snap be
 * done by setting minutes on a `Date` and never touching the hour, which in
 * turn is what keeps it away from daylight-saving arithmetic entirely.
 * `slotAt` makes the same assumption for its 30.
 */
export function snapMs(ms: number, intervalMs: number): number {
  const intervalMin = intervalMs / 60_000;
  const at = new Date(ms);
  // Minutes past the hour, carrying the sub-minute part so a value just short
  // of a boundary rounds on to it rather than being truncated back.
  const within = at.getMinutes() + at.getSeconds() / 60 + at.getMilliseconds() / 60_000;
  // `setMinutes` handles a rounded 60 by rolling the hour itself, in local
  // time, which is exactly what is wanted and is why the hour is never
  // computed here.
  at.setMinutes(Math.round(within / intervalMin) * intervalMin, 0, 0);
  return at.getTime();
}

/**
 * The span an event lands on when it is dragged.
 *
 * `dyFrac` is how far the pointer travelled down the column as a fraction of
 * its height, `dayMs` the span of the day it is being dragged in, and `dxCols`
 * how many day columns it crossed. `dayCols` is the number of columns in the
 * view, and bounds the horizontal movement so a drag cannot leave the week it
 * is in.
 *
 * **The duration never changes.** The end is the new start plus the span that
 * went in, and is deliberately *not* recomputed from the pointer: a move that
 * derived both ends independently would resize the event by however much the
 * two roundings disagreed, silently, while still landing where the test looked.
 *
 * **A day is a civil day.** `setDate` keeps the wall-clock time and moves the
 * date, so dragging a 09:00 meeting across a spring-forward leaves it at 09:00
 * — twenty-three hours later, not twenty-four. Adding `dxCols * 86400000`
 * would move it to 10:00, which is the defect this project has closed twice
 * elsewhere and has no business reintroducing through a gesture.
 *
 * A move of nothing returns the very instants it was given, rather than a
 * value that merely reads the same: §4 says putting an event back where it came
 * from must be free, and a civil round trip through a repeated hour could
 * otherwise hand back the other pass of it.
 */
export function spanForMove(
  origin: Span,
  dyFrac: number,
  dayMs: number,
  dayCols: number,
  dxCols: number,
  snapInterval: number,
): Span {
  const duration = origin.endMs - origin.startMs;

  // Clamped rather than refused: a pointer dragged off the right-hand edge
  // should pin to the last column, not throw away the gesture.
  const cols = clamp(dxCols, -(dayCols - 1), dayCols - 1);

  const moved = dyFrac === 0 && cols === 0
    ? origin.startMs
    : addDays(snapMs(origin.startMs + dyFrac * dayMs, snapInterval), cols);

  return { startMs: moved, endMs: moved + duration };
}

/**
 * The span an event lands on when one of its edges is dragged.
 *
 * The other edge stays exactly where it was — spec §5 — so a resize is never
 * also a move.
 *
 * **A resize may not invert the event.** Dragging an edge past its opposite
 * clamps to a minimum span rather than producing a negative one:
 * `endAfterStart` already refuses an inverted span in the form, and a grid able
 * to construct one would produce a block the form then rejects. That
 * inconsistency is the defect, not the negative number.
 *
 * The minimum is the snap interval itself, deliberately rather than a second
 * constant. The smallest span the grid can express is the smallest one it
 * should be able to produce, and a separate number would be one more thing to
 * keep in step with §5.
 */
export function spanForResize(
  origin: Span,
  edge: 'start' | 'end',
  dyFrac: number,
  dayMs: number,
  snapInterval: number,
): Span {
  const minimum = snapInterval;

  if (edge === 'start') {
    const wanted = snapMs(origin.startMs + dyFrac * dayMs, snapInterval);
    return { startMs: Math.min(wanted, origin.endMs - minimum), endMs: origin.endMs };
  }

  const wanted = snapMs(origin.endMs + dyFrac * dayMs, snapInterval);
  return { startMs: origin.startMs, endMs: Math.max(wanted, origin.startMs + minimum) };
}

/** `n` whole **civil** days on from `ms`, keeping the local wall-clock time. */
function addDays(ms: number, n: number): number {
  if (n === 0) return ms;
  const at = new Date(ms);
  at.setDate(at.getDate() + n);
  return at.getTime();
}

const clamp = (n: number, lo: number, hi: number): number => Math.min(Math.max(n, lo), hi);
